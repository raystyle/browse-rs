//! browser-harness-js 片段方言的语法分析器（纯函数）。
//!
//! 移植自 browser-harness-rs `src/js_host.rs` 的解析半边，独立成模块便于单测。
//! 方言支持：字面量、对象、数组、成员、下标、`await`、`const/let/var`、
//! `return`、`//` 注释。不支持：函数字面量、`if/for/while`、模板字符串；
//! 这些在解析期就报错并提示「页面逻辑放 `Runtime.evaluate` 的 expression」。

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

/// 方言的顶层语句形态：声明、表达式或 return。
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// `const/let/var name = expr`
    Let {
        /// 变量名。
        name: String,
        /// 右值表达式。
        expr: Expr,
    },
    /// 裸表达式语句。
    Expr(
        /// 表达式。
        Expr,
    ),
    /// `return expr`（提前返回片段结果）。
    Return(
        /// 返回值表达式。
        Expr,
    ),
}

/// 方言的表达式节点，构成求值树。
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// JSON 字面量。
    Lit(Value),
    /// 标识符引用。
    Ident(String),
    /// `await expr`
    Await(Box<Expr>),
    /// `obj.prop`
    Member {
        /// 对象表达式。
        obj: Box<Expr>,
        /// 属性名。
        prop: String,
    },
    /// `obj[index]`
    Index {
        /// 对象表达式。
        obj: Box<Expr>,
        /// 下标表达式。
        index: Box<Expr>,
    },
    /// `callee(args...)`
    Call {
        /// 被调表达式。
        callee: Box<Expr>,
        /// 实参列表。
        args: Vec<Expr>,
    },
    /// 对象字面量 `{k: v, ...}`
    Object(Vec<(String, Expr)>),
    /// 数组字面量 `[a, b]`
    Array(Vec<Expr>),
}

/// 不支持语式的统一报错（CTA：诊断 + 下一步）。
///
/// 保留关键字 `Runtime.evaluate`，契约测试锁它。
const UNSUPPORTED_HINT: &str = "方言不支持该语法（if/for/while/函数/模板字符串）；下一步：页面逻辑放 Runtime.evaluate 的 expression 字符串，宿主侧只留 CDP 调用与取值";

/// 把字节偏移换算成「行L:列C」（错误定位用，CTA 的一半是位置）。
///
/// # Examples
///
/// ```
/// let two_lines = concat!("ab", '\n', "cd");
/// assert_eq!(browse_core::parser::loc(two_lines, 4), "行2:列2");
/// assert_eq!(browse_core::parser::loc("abc", 0), "行1:列1");
/// ```
pub fn loc(src: &str, pos: usize) -> String {
    let head = &src[..pos.min(src.len())];
    let line = head.matches('\n').count() + 1;
    let col = head
        .rsplit('\n')
        .next()
        .map(str::chars)
        .map(|c| c.count())
        .unwrap_or(0)
        + 1;
    format!("行{line}:列{col}")
}

/// 把整段片段解析成语句列表（先剥 `//` 注释）。
///
/// # Errors
///
/// - 含方言外语法（if/for/while/函数/模板字符串，报错带下一步 CTA）。
/// - token 残缺（未闭合字符串、缺 `=` / `]` / `,` 等），错误信息带停住的位置。
/// - 末尾有解析不掉的余量。
///
/// # Examples
///
/// ```
/// use browse_core::parser::{parse_script, Stmt};
///
/// let stmts = parse_script("const n = 1").unwrap();
/// assert!(matches!(stmts[0], Stmt::Let { .. }));
/// assert!(parse_script("if (x) { y() }").is_err()); // 方言外
/// ```
pub fn parse_script(source: &str) -> Result<Vec<Stmt>> {
    let src = strip_comments(source);
    let src = src.trim();
    if src.is_empty() {
        return Ok(Vec::new());
    }
    let mut p = Parser::new(src);
    let stmts = p.parse_script()?;
    p.skip_ws();
    if p.pos < p.src.len() {
        bail!(
            "未解析完，停在「{}」（{}）；下一步：检查该处附近的括号/引号是否闭合",
            p.src[p.pos..].chars().take(40).collect::<String>(),
            loc(p.src, p.pos)
        );
    }
    Ok(stmts)
}

/// 判断片段括号是否配平（stdin/TTY 增量读入用：配平才送求值）。
///
/// # Examples
///
/// ```
/// assert!(browse_core::snippet_complete("await session.use(tabs[0].targetId)"));
/// assert!(!browse_core::snippet_complete("const x = {a: 1"));
/// ```
pub fn snippet_complete(src: &str) -> bool {
    let s = src.trim();
    if s.is_empty() {
        return false;
    }
    depth_ok(s)
}

fn depth_ok(s: &str) -> bool {
    let mut par = 0i32;
    let mut br = 0i32;
    let mut sq = 0i32;
    let mut quote: Option<char> = None;
    let mut esc = false;
    for c in s.chars() {
        if let Some(q) = quote {
            if esc {
                esc = false;
                continue;
            }
            if c == '\\' {
                esc = true;
                continue;
            }
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' => quote = Some(c),
            '(' => par += 1,
            ')' => par -= 1,
            '{' => br += 1,
            '}' => br -= 1,
            '[' => sq += 1,
            ']' => sq -= 1,
            _ => {}
        }
    }
    quote.is_none() && par <= 0 && br <= 0 && sq <= 0
}

/// 把 `//` 行注释剥掉，保留字符串字面量里的 `//`。
///
/// # Examples
///
/// ```
/// assert_eq!(browse_core::parser::strip_comments("a // 尾注\nb"), "a \nb");
/// assert_eq!(browse_core::parser::strip_comments(r#""http://x""#), r#""http://x""#);
/// ```
pub fn strip_comments(s: &str) -> String {
    let mut out = String::new();
    let mut quote: Option<char> = None;
    let mut esc = false;
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = quote {
            out.push(c);
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '\'' || c == '"' {
            quote = Some(c);
        }
        out.push(c);
        i += 1;
    }
    out
}

/// 把语句列表回显成源码（诊断与 doctest 用，非规范格式化器）。
///
/// # Examples
///
/// ```
/// use browse_core::parser::{parse_script, render};
/// let stmts = parse_script("await session.Page.navigate({url:\"https://example.com\"})").unwrap();
/// assert_eq!(render(&stmts), r#"await session.Page.navigate({url: "https://example.com"})"#);
/// ```
pub fn render(stmts: &[Stmt]) -> String {
    let parts: Vec<String> = stmts.iter().map(render_stmt).collect();
    parts.join(";\n")
}

fn render_stmt(s: &Stmt) -> String {
    match s {
        Stmt::Let { name, expr } => format!("const {name} = {}", render_expr(expr)),
        Stmt::Expr(expr) => render_expr(expr),
        Stmt::Return(expr) => format!("return {}", render_expr(expr)),
    }
}

fn render_expr(e: &Expr) -> String {
    match e {
        Expr::Lit(v) => match v {
            Value::String(s) => format!("{s:?}"),
            other => other.to_string(),
        },
        Expr::Ident(n) => n.clone(),
        Expr::Await(inner) => format!("await {}", render_expr(inner)),
        Expr::Member { obj, prop } => format!("{}.{}", render_expr(obj), prop),
        Expr::Index { obj, index } => format!("{}[{}]", render_expr(obj), render_expr(index)),
        Expr::Call { callee, args } => {
            let argv: Vec<String> = args.iter().map(render_expr).collect();
            format!("{}({})", render_expr(callee), argv.join(", "))
        }
        Expr::Object(kvs) => {
            let pairs: Vec<String> = kvs
                .iter()
                .map(|(k, v)| format!("{k}: {}", render_expr(v)))
                .collect();
            format!("{{{}}}", pairs.join(", "))
        }
        Expr::Array(xs) => {
            let items: Vec<String> = xs.iter().map(render_expr).collect();
            format!("[{}]", items.join(", "))
        }
    }
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Self {
        Self { src, pos: 0 }
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.pos += c.len_utf8();
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    /// 当前位置（错误消息用）。
    fn here(&self) -> String {
        loc(self.src, self.pos)
    }

    fn eat(&mut self, s: &str) -> bool {
        self.skip_ws();
        if self.src[self.pos..].starts_with(s) {
            self.pos += s.len();
            true
        } else {
            false
        }
    }

    fn parse_script(&mut self) -> Result<Vec<Stmt>> {
        let mut out = Vec::new();
        loop {
            self.skip_ws();
            if self.pos >= self.src.len() {
                break;
            }
            if self.eat(";") {
                continue;
            }
            out.push(self.parse_stmt()?);
            self.eat(";");
        }
        Ok(out)
    }

    fn parse_stmt(&mut self) -> Result<Stmt> {
        self.skip_ws();
        if self.peek() == Some('`') {
            bail!("{UNSUPPORTED_HINT}（模板字符串，{}）", self.here());
        }
        if let Some(kw) = self.peek_keyword()
            && matches!(kw, "if" | "for" | "while" | "function" | "class")
        {
            bail!("{UNSUPPORTED_HINT}（发现 {kw}，{}）", self.here());
        }
        if self.eat("return") {
            self.skip_ws();
            if self.peek() == Some(';') || self.pos >= self.src.len() {
                return Ok(Stmt::Return(Expr::Lit(Value::Null)));
            }
            return Ok(Stmt::Return(self.parse_expr()?));
        }
        if self.eat("const") || self.eat("let") || self.eat("var") {
            let name = self.parse_ident()?;
            if !self.eat("=") {
                bail!(
                    "期望 =（声明 {name} 后，{}）；下一步：补成 const {name} = <值>",
                    self.here()
                );
            }
            return Ok(Stmt::Let {
                name,
                expr: self.parse_expr()?,
            });
        }
        Ok(Stmt::Expr(self.parse_expr()?))
    }

    fn peek_keyword(&self) -> Option<&'a str> {
        for kw in ["if", "for", "while", "function", "class"] {
            let rest = &self.src[self.pos..];
            if rest.starts_with(kw) {
                let boundary = match rest.strip_prefix(kw).and_then(|r| r.chars().next()) {
                    None => true,
                    Some(c) => c.is_whitespace() || c == '(' || c == '{',
                };
                if boundary {
                    return Some(kw);
                }
            }
        }
        None
    }

    fn parse_expr(&mut self) -> Result<Expr> {
        self.skip_ws();
        if self.eat("await") {
            return Ok(Expr::Await(Box::new(self.parse_expr()?)));
        }
        self.parse_member()
    }

    fn parse_member(&mut self) -> Result<Expr> {
        let mut e = self.parse_primary()?;
        loop {
            self.skip_ws();
            if self.eat(".") {
                let prop = self.parse_ident()?;
                e = Expr::Member {
                    obj: Box::new(e),
                    prop,
                };
                continue;
            }
            if self.eat("[") {
                let index = self.parse_expr()?;
                if !self.eat("]") {
                    bail!("期望 ]（{}）；下一步：补齐下标的闭合中括号", self.here());
                }
                e = Expr::Index {
                    obj: Box::new(e),
                    index: Box::new(index),
                };
                continue;
            }
            if self.eat("(") {
                let args = self.parse_args()?;
                e = Expr::Call {
                    callee: Box::new(e),
                    args,
                };
                continue;
            }
            if self.src[self.pos..].starts_with("=>") {
                bail!("{UNSUPPORTED_HINT}（箭头函数，{}）", self.here());
            }
            break;
        }
        Ok(e)
    }

    fn parse_args(&mut self) -> Result<Vec<Expr>> {
        let mut args = Vec::new();
        self.skip_ws();
        if self.eat(")") {
            return Ok(args);
        }
        loop {
            args.push(self.parse_expr()?);
            self.skip_ws();
            if self.eat(")") {
                break;
            }
            if !self.eat(",") {
                bail!(
                    "期望 , 或 )（{}）；下一步：实参之间用逗号，末尾闭括号",
                    self.here()
                );
            }
            self.skip_ws();
            if self.peek() == Some(')') {
                self.eat(")");
                break;
            }
        }
        Ok(args)
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        self.skip_ws();
        if self.eat("true") {
            return Ok(Expr::Lit(json!(true)));
        }
        if self.eat("false") {
            return Ok(Expr::Lit(json!(false)));
        }
        if self.eat("null") || self.eat("undefined") {
            return Ok(Expr::Lit(Value::Null));
        }
        if self.peek() == Some('`') {
            bail!("{UNSUPPORTED_HINT}（模板字符串，{}）", self.here());
        }
        if self.eat("{") {
            return self.parse_object();
        }
        if self.eat("[") {
            return self.parse_array();
        }
        if self.eat("(") {
            let e = self.parse_expr()?;
            if !self.eat(")") {
                bail!(
                    "期望 )（{}）；下一步：补齐括号；stdin 模式会攒到括号配平才发送",
                    self.here()
                );
            }
            return Ok(e);
        }
        match self.peek() {
            Some('"') | Some('\'') => Ok(Expr::Lit(Value::String(self.parse_string()?))),
            Some(c) if c.is_ascii_digit() || c == '-' => Ok(Expr::Lit(self.parse_number()?)),
            Some(c) if is_ident_start(c) => Ok(Expr::Ident(self.parse_ident()?)),
            other => bail!(
                "意外 token {other:?}（{}）；方言是值语言，没有运算符；下一步：计算放 Runtime.evaluate 的 expression，宿主侧直接写字面量",
                self.here()
            ),
        }
    }

    fn parse_object(&mut self) -> Result<Expr> {
        let mut kvs = Vec::new();
        loop {
            self.skip_ws();
            if self.eat("}") {
                break;
            }
            let key = if self.peek() == Some('"') || self.peek() == Some('\'') {
                self.parse_string()?
            } else {
                self.parse_ident()?
            };
            if !self.eat(":") {
                bail!(
                    "期望 :（键 {key} 后，{}）；下一步：对象字面量写成 {{key: value}}",
                    self.here()
                );
            }
            let val = self.parse_expr()?;
            kvs.push((key, val));
            self.skip_ws();
            self.eat(",");
            self.skip_ws();
            if self.peek() == Some('}') {
                self.eat("}");
                break;
            }
        }
        Ok(Expr::Object(kvs))
    }

    fn parse_array(&mut self) -> Result<Expr> {
        let mut xs = Vec::new();
        loop {
            self.skip_ws();
            if self.eat("]") {
                break;
            }
            xs.push(self.parse_expr()?);
            self.skip_ws();
            self.eat(",");
            self.skip_ws();
            if self.peek() == Some(']') {
                self.eat("]");
                break;
            }
        }
        Ok(Expr::Array(xs))
    }

    fn parse_ident(&mut self) -> Result<String> {
        self.skip_ws();
        let start = self.pos;
        if let Some(c) = self.peek() {
            if !is_ident_start(c) {
                bail!(
                    "期望标识符（{}）；下一步：成员访问写 .prop，属性名限字母数字下划线",
                    self.here()
                );
            }
            self.pos += c.len_utf8();
        }
        while let Some(c) = self.peek() {
            if is_ident_continue(c) {
                self.pos += c.len_utf8();
            } else {
                break;
            }
        }
        Ok(self.src[start..self.pos].to_string())
    }

    fn parse_string(&mut self) -> Result<String> {
        self.skip_ws();
        let q = self.peek().ok_or_else(|| {
            anyhow!(
                "期望字符串（{}）；下一步：字符串用成对单/双引号",
                self.here()
            )
        })?;
        if q != '"' && q != '\'' {
            bail!(
                "期望字符串（{}）；下一步：字符串用成对单/双引号",
                self.here()
            );
        }
        self.pos += 1;
        let mut out = String::new();
        loop {
            let c = self
                .peek()
                .ok_or_else(|| anyhow!("未闭合字符串（{}）；下一步：补上结尾引号", self.here()))?;
            self.pos += c.len_utf8();
            if c == q {
                break;
            }
            if c == '\\' {
                let n = self.peek().ok_or_else(|| {
                    anyhow!("未闭合字符串（{}）；下一步：补上结尾引号", self.here())
                })?;
                self.pos += n.len_utf8();
                out.push(match n {
                    'n' => '\n',
                    't' => '\t',
                    other => other,
                });
            } else {
                out.push(c);
            }
        }
        Ok(out)
    }

    fn parse_number(&mut self) -> Result<Value> {
        self.skip_ws();
        let start = self.pos;
        if self.peek() == Some('-') {
            self.pos += 1;
        }
        while matches!(self.peek(), Some(c) if c.is_ascii_digit() || c == '.') {
            self.pos += 1;
        }
        let s = &self.src[start..self.pos];
        if s.contains('.') {
            s.parse::<f64>().map(|v| json!(v)).map_err(|_| {
                anyhow!(
                    "数字字面量 {s} 解析不了（{}）；下一步：检查小数点与位数",
                    self.here()
                )
            })
        } else {
            s.parse::<i64>()
                .map(|v| json!(v))
                .map_err(|e| {
                    // 按错误分型：真溢出才给大数出路，形态错（如裸负号）归因形态
                    let hint =
                        if matches!(e.kind(), std::num::IntErrorKind::PosOverflow | std::num::IntErrorKind::NegOverflow) {
                            format!("整数 {s} 超出 64 位范围；下一步：超范围大数改字符串字面量承载，页面侧大整数走 session.Runtime.evaluate 的 BigInt")
                        } else {
                            format!("数字字面量 {s} 形态不对；下一步：此处应是数字（负号后须跟数字）")
                        };
                    anyhow!("{hint}（{}）", self.here())
                })
        }
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c == '$'
}

fn is_ident_continue(c: char) -> bool {
    is_ident_start(c) || c.is_ascii_digit()
}
