# cdp::pipe

匿名管道薄封装。std 的 anonymous pipe（`std::io::pipe`）至 1.98 仍未稳定，
Windows 直接用 `CreatePipe`，POSIX 用 `pipe(2)`。

句柄/fd 存 `usize` 以获得 `Send + Sync`；阻塞 IO 由 [`crate::Session`] 的
管道泵在 blocking 线程池上跑。

## Functions

- `anon_pair` — 建一对匿名管道：`(读端, 写端)`。
- `set_inheritable` — POSIX 恒成功（Windows 对应物才可能失败）。

## Types

- `PipeReader` — CDP 管道的读端，启动器持有，读 chrome 写出的字节流。
- `PipeWriter` — CDP 管道的写端，启动器持有，往 chrome 写 CDP 字节流。

