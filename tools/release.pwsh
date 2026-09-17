#requires -Version 7
<#
release.pwsh —— browse 本地发布面（批二，总台核准 2026-09-17）

对齐 build-release 标准三段式的第一二段：本地编译打包 + gh release 直发
--latest 禁 draft；第三段（R2 双段播种）在 .github/workflows/release.yml
（批一），由 release published 事件自动接力，本脚本不做镜像面。

流程（每步失败即红，见 Run 器）：
  1 版本一致性闸（tag 对 Cargo.toml workspace 版本）
  2 测试闸（cargo test --locked）
  3 本地编译：linux 本职 + win-gnu 交叉（mingw）；mac arm64 在 lan-mac 实机
    （rsync 源树、构建、取回；标准：mac 形在 mac 实机）
  4 打包：单顶层目录 = browse 二进制 + README + LICENSE 双件；
    win 形 zip 他形 tar.gz；逐包 .sha256 边车（sha256sum 原生格式）
  5 跨宿主断言与解包冒烟：file 三断言（ELF/PE/Mach-O）；
    三包各解出跑 browse --version 对 tag 逐字（win 经 interop、mac 经 ssh）
  6 gh release 直发 dist 全量 --latest

用法：pwsh tools/release.pwsh -Tag v0.4.2
前置：远端 tag 已推；gh 已登录；ssh lan-mac 可达；mingw-w64 与 zip 在位。
#>
[CmdletBinding()]
param(
    # 要发布的版本 tag（v 前缀形，与 Cargo.toml workspace 版本一致性闸在脚本内过）
    [Parameter(Mandatory)]
    [ValidatePattern('^v\d+\.\d+\.\d+')]
    [string]$Tag,

    # win 冒烟的中转目录（Windows 侧可达路径；interop 直调面）
    [string]$WinSmokeDir = '/mnt/c/Users/ray/bin',

    # mac 实机（构建岗；标准：mac 形在 mac 实机）
    [string]$MacHost = 'lan-mac'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 3.0

# 原生命令执行器：pwsh 的 Stop 不拦原生命令退出码，非零显式红
function Run([string]$Exe, [string[]]$Argv, [string]$What) {
    & $Exe @Argv
    if ($LASTEXITCODE -ne 0) {
        throw "红：$What（exit $LASTEXITCODE）"
    }
}

$Ver = $Tag.TrimStart('v')
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    # 1 版本一致性闸
    $hit = Select-String -Path Cargo.toml -Pattern '^version = "([^"]+)"' | Select-Object -First 1
    $manifest = $hit.Matches[0].Groups[1].Value
    if ($manifest -ne $Ver) {
        throw "版本闸红：Cargo.toml=$manifest != tag=$Ver"
    }

    # 1b 发布锚链预检（评审二轮 F 修，照 hst/ark 同形）：洁净闸 + 预检三件。
    # CI 退场后本脚本是唯一正式发布口，tag 与提交与产物的锚链要机器保障：
    # 远端无 tag 时 gh release create 会默认在分支 HEAD 补建 tag，锚链即断。
    $dirty = & git status --porcelain
    if ($LASTEXITCODE -ne 0 -or "$dirty".Trim()) { throw "红：工作树不洁净（发布要求洁净树）" }
    $head = (& git rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0 -or -not $head) { throw "红：git rev-parse 失败" }
    $remote = & git ls-remote origin "refs/tags/$Tag" 2>$null
    if ($LASTEXITCODE -ne 0) { throw "红：git ls-remote 失败" }
    if ("$remote" -notmatch "refs/tags/$Tag") {
        throw "锚链红：远端无 $Tag；下一步：git tag $Tag && git push origin $Tag 后重跑"
    }
    if ("$remote" -notmatch "^$head") {
        throw "锚链红：远端 $Tag 不指向本提交（$head）；下一步：核 tag 来源后重推 tag"
    }
    & gh auth status *> $null
    if ($LASTEXITCODE -ne 0) { throw "红：gh 未登录（gh auth login）" }
    & gh release view $Tag *> $null
    if ($LASTEXITCODE -eq 0) { throw "红：release $Tag 已存在；补挂资产走 gh release upload，勿重建" }

    # 2 测试闸
    Run cargo @('test', '--locked') '测试闸'

    # 3 本地编译
    Run cargo @('build', '--release', '--locked', '--target', 'x86_64-unknown-linux-gnu', '-p', 'browse-cli') 'linux 构建'
    Run cargo @('build', '--release', '--locked', '--target', 'x86_64-pc-windows-gnu', '-p', 'browse-cli') 'win-gnu 交叉构建'
    Run rsync @('-a', '--delete', '--exclude', 'target', '--exclude', '.git', "$root/", "${MacHost}:~/browse-rs/") 'mac 树同步'
    Run ssh @($MacHost, 'cd ~/browse-rs && cargo build --release --locked --target aarch64-apple-darwin -p browse-cli') 'mac 构建'
    $macTmp = Join-Path ([System.IO.Path]::GetTempPath()) "browse-${Tag}-mac"
    Run scp @('-q', "${MacHost}:~/browse-rs/target/aarch64-apple-darwin/release/browse", $macTmp) 'mac 二进制取回'

    # 4 打包
    $dist = Join-Path $root 'dist'
    if (Test-Path $dist) { Remove-Item -Recurse -Force $dist }
    New-Item -ItemType Directory -Path $dist | Out-Null
    # Inner = 包内二进制名（终审 F1 修：mac 取回的临时名不得原名入包，
    # 管理方消费的是 browse；三目标显式内层名杜绝来源名漂移）
    $targets = @(
        @{ Triple = 'x86_64-unknown-linux-gnu'; Archive = 'tar.gz'; Bin = "target/x86_64-unknown-linux-gnu/release/browse"; Inner = 'browse' },
        @{ Triple = 'x86_64-pc-windows-gnu'; Archive = 'zip'; Bin = "target/x86_64-pc-windows-gnu/release/browse.exe"; Inner = 'browse.exe' },
        @{ Triple = 'aarch64-apple-darwin'; Archive = 'tar.gz'; Bin = $macTmp; Inner = 'browse' }
    )
    foreach ($t in $targets) {
        $d = "browse-${Tag}-$($t.Triple)"
        New-Item -ItemType Directory -Path (Join-Path $dist $d) | Out-Null
        Copy-Item $t.Bin (Join-Path (Join-Path $dist $d) $t.Inner)
        Copy-Item README.md, LICENSE-MIT, LICENSE-APACHE (Join-Path $dist $d)
        Push-Location $dist
        try {
            if ($t.Archive -eq 'zip') {
                Run zip @('-q', '-r', "$d.zip", $d) "打包 $d"
            }
            else {
                Run tar @('czf', "$d.tar.gz", $d) "打包 $d"
            }
            $sum = & sha256sum "$d.$($t.Archive)"
            if ($LASTEXITCODE -ne 0) { throw "红：边车（$d）" }
            "$sum" | Set-Content "$d.$($t.Archive).sha256"
        }
        finally { Pop-Location }
        Remove-Item -Recurse -Force (Join-Path $dist $d)
    }

    # 5 跨宿主断言与解包冒烟
    $smoke = Join-Path ([System.IO.Path]::GetTempPath()) "browse-${Tag}-smoke"
    if (Test-Path $smoke) { Remove-Item -Recurse -Force $smoke }
    New-Item -ItemType Directory -Path $smoke | Out-Null

    # file 三断言（ELF / PE / Mach-O）
    $magics = @{
        'x86_64-unknown-linux-gnu' = 'ELF'
        'x86_64-pc-windows-gnu' = 'PE'
        'aarch64-apple-darwin' = 'Mach-O'
    }
    foreach ($t in $targets) {
        $bin = $t.Bin
        $out = & file -b $bin
        if ($LASTEXITCODE -ne 0) { throw "红：file 断言（$($t.Triple)）" }
        if ($out -notmatch $magics[$t.Triple]) {
            throw "file 断言红：$($t.Triple) 期望 $($magics[$t.Triple])，得 $out"
        }
    }

    # linux：本地解包直跑
    $dl = "browse-${Tag}-x86_64-unknown-linux-gnu"
    Run tar @('xzf', "$dist/$dl.tar.gz", '-C', $smoke) 'linux 解包'
    $got = & "$smoke/$dl/browse" --version
    if ("$got" -cne "browse $Ver") { throw "linux 冒烟红：$got" }

    # win：解包取 exe 经 interop 直跑
    $dw = "browse-${Tag}-x86_64-pc-windows-gnu"
    Run unzip @('-q', "$dist/$dw.zip", '-d', $smoke) 'win 解包'
    $winExe = Join-Path $WinSmokeDir "browse-rel-smoke.exe"
    Copy-Item "$smoke/$dw/browse.exe" $winExe -Force
    try {
        $winExeWin = ($winExe -replace '^/mnt/c', 'C:').Replace('/', '\')
        $got = & cmd.exe /c "$winExeWin --version"
        if ("$got".Trim() -cne "browse $Ver") { throw "win 冒烟红：$got" }
    }
    finally { Remove-Item -Force $winExe -ErrorAction SilentlyContinue }

    # mac：包推 mac 实机解包直跑
    $dm = "browse-${Tag}-aarch64-apple-darwin"
    Run scp @('-q', "$dist/$dm.tar.gz", "${MacHost}:/tmp/") 'mac 冒烟件推送'
    $got = & ssh $MacHost "rm -rf /tmp/rel-smoke && mkdir /tmp/rel-smoke && tar xzf /tmp/$dm.tar.gz -C /tmp/rel-smoke && /tmp/rel-smoke/$dm/browse --version && rm -rf /tmp/rel-smoke /tmp/$dm.tar.gz"
    if ("$got".Trim() -cne "browse $Ver") { throw "mac 冒烟红：$got" }

    # 6 gh release 直发（--latest 禁 draft；多发布工具各建 release 是抢 tag 事故源，发布器唯一）
    $assets = (Get-ChildItem $dist | ForEach-Object FullName)
    $ghArgv = @('release', 'create', $Tag) + $assets + @(
        '--latest', '--title', "browse $Tag",
        '--notes', "正式版 $Tag（本地产线构建：linux 本职加 win-gnu 交叉加 mac 实机）。三平台包加 sha256 边车；镜像双段（browse/$Ver/ 加 stable/）由播种流水自动接力。agent 发现通道：browse --llms。"
    )
    Run gh $ghArgv 'gh release 直发'

    Write-Host "发布毕：$Tag（dist 六件；播种流水由 release published 事件接力）"
}
finally {
    Pop-Location
}
