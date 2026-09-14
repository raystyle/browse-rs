# cdp::pipe

匿名管道薄封装。std 的 anonymous pipe（`std::io::pipe`）至 1.98 仍未稳定，
Windows 直接用 `CreatePipe`；POSIX 留桩（管道通道暂仅 Windows，走端口态）。

句柄存 `usize` 以获得 `Send + Sync`；阻塞 IO 由 [`crate::Session`] 的
管道泵在 blocking 线程池上跑。

## Functions

- `anon_pair` — 建一对匿名管道（默认不可继承）：`(读端, 写端)`。
- `set_inheritable` — 把句柄标为（不）可被子进程继承（`HANDLE_FLAG_INHERIT`）。

## Types

- `PipeReader` — 管道读端（启动器持有，读 chrome 写出的 CDP）。
- `PipeWriter` — 管道写端（启动器持有，往 chrome 写 CDP）。

