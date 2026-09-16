//! 匿名管道薄封装。std 的 anonymous pipe（`std::io::pipe`）至 1.98 仍未稳定，
//! Windows 直接用 `CreatePipe`，POSIX 用 `pipe(2)`。
//!
//! 句柄/fd 存 `usize` 以获得 `Send + Sync`；阻塞 IO 由 [`crate::Session`] 的
//! 管道泵在 blocking 线程池上跑。

use anyhow::{Result, anyhow};

/// CDP 管道的读端，启动器持有，读 chrome 写出的字节流。
pub struct PipeReader {
    /// OS 句柄（Windows HANDLE / POSIX fd）。
    pub h: usize,
}

/// CDP 管道的写端，启动器持有，往 chrome 写 CDP 字节流。
pub struct PipeWriter {
    /// OS 句柄（Windows HANDLE / POSIX fd）。
    pub h: usize,
}

#[cfg(windows)]
impl Drop for PipeReader {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.h as _) };
    }
}

#[cfg(windows)]
impl Drop for PipeWriter {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.h as _) };
    }
}

#[cfg(windows)]
impl std::io::Read for PipeReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        use windows_sys::Win32::Foundation::{ERROR_BROKEN_PIPE, GetLastError};
        use windows_sys::Win32::Storage::FileSystem::ReadFile;
        let mut n: u32 = 0;
        let ok = unsafe {
            ReadFile(
                self.h as _,
                buf.as_mut_ptr().cast(),
                buf.len() as u32,
                &mut n,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            let err = unsafe { GetLastError() };
            if err == ERROR_BROKEN_PIPE {
                return Ok(0); // 对端关了：EOF
            }
            return Err(std::io::Error::from_raw_os_error(err as i32));
        }
        Ok(n as usize)
    }
}

#[cfg(windows)]
impl std::io::Write for PipeWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        use windows_sys::Win32::Foundation::GetLastError;
        use windows_sys::Win32::Storage::FileSystem::WriteFile;
        let mut n: u32 = 0;
        let ok = unsafe {
            WriteFile(
                self.h as _,
                buf.as_ptr().cast(),
                buf.len() as u32,
                &mut n,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(std::io::Error::from_raw_os_error(
                unsafe { GetLastError() } as i32
            ));
        }
        Ok(n as usize)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(windows)]
impl std::os::windows::io::AsRawHandle for PipeReader {
    fn as_raw_handle(&self) -> std::os::windows::io::RawHandle {
        self.h as _
    }
}

#[cfg(windows)]
impl std::os::windows::io::AsRawHandle for PipeWriter {
    fn as_raw_handle(&self) -> std::os::windows::io::RawHandle {
        self.h as _
    }
}

#[cfg(not(windows))]
impl Drop for PipeReader {
    fn drop(&mut self) {
        unsafe { libc::close(self.h as i32) };
    }
}

#[cfg(not(windows))]
impl Drop for PipeWriter {
    fn drop(&mut self) {
        unsafe { libc::close(self.h as i32) };
    }
}

#[cfg(not(windows))]
impl std::io::Read for PipeReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            let n = unsafe { libc::read(self.h as i32, buf.as_mut_ptr().cast(), buf.len()) };
            if n >= 0 {
                return Ok(n as usize); // 0 = 对端关闭（EOF）
            }
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
    }
}

#[cfg(not(windows))]
impl std::io::Write for PipeWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = unsafe { libc::write(self.h as i32, buf.as_ptr().cast(), buf.len()) };
        if n < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(n as usize)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// 建一对匿名管道（默认不可继承）：`(读端, 写端)`。
///
/// # Errors
///
/// `CreatePipe` 失败（错误码进上下文）。
#[cfg(windows)]
pub fn anon_pair() -> Result<(PipeReader, PipeWriter)> {
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::Pipes::CreatePipe;
    let mut r = std::ptr::null_mut();
    let mut w = std::ptr::null_mut();
    let ok = unsafe { CreatePipe(&mut r, &mut w, std::ptr::null(), 0) };
    if ok == 0 {
        return Err(anyhow!("CreatePipe 失败：{}", unsafe { GetLastError() }));
    }
    Ok((PipeReader { h: r as usize }, PipeWriter { h: w as usize }))
}

/// 建一对匿名管道：`(读端, 写端)`。
///
/// POSIX fd 跨 fork/exec 继承（非 CLOEXEC），布线到固定 fd 由 spawn 侧的
/// pre_exec 做。
///
/// # Errors
///
/// `pipe(2)` 失败。
#[cfg(not(windows))]
pub fn anon_pair() -> Result<(PipeReader, PipeWriter)> {
    let mut fds = [0i32; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(anyhow!("pipe(2) 失败：{}", std::io::Error::last_os_error()));
    }
    Ok((
        PipeReader { h: fds[0] as usize },
        PipeWriter { h: fds[1] as usize },
    ))
}

/// 把句柄标为（不）可被子进程继承（`HANDLE_FLAG_INHERIT`）。
///
/// # Errors
///
/// `SetHandleInformation` 失败。
#[cfg(windows)]
pub fn set_inheritable(h: usize, on: bool) -> Result<()> {
    const HANDLE_FLAG_INHERIT: u32 = 0x1;
    let ok = unsafe {
        windows_sys::Win32::Foundation::SetHandleInformation(
            h as _,
            HANDLE_FLAG_INHERIT,
            if on { HANDLE_FLAG_INHERIT } else { 0 },
        )
    };
    if ok == 0 {
        return Err(anyhow!("SetHandleInformation 失败（句柄 {h}）"));
    }
    Ok(())
}

/// POSIX 恒成功（Windows 对应物才可能失败）。
///
/// fd 跨 fork/exec 默认继承（非 CLOEXEC），布到固定 fd 3/4 由 spawn 的
/// pre_exec 负责。
///
/// # Errors
///
/// 恒不报错（Windows 对应物才可能失败）。
#[cfg(not(windows))]
pub fn set_inheritable(_h: usize, _on: bool) -> Result<()> {
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// POSIX 管道往返：NUL 也能过、排空后写端 drop 读端见 EOF
    /// （CI ubuntu 跑；Windows 本机跳过）。
    #[test]
    fn posix_pipe_roundtrip_and_eof() {
        let (mut r, mut w) = anon_pair().expect("pipe(2)");
        w.write_all(b"hello\0world").expect("write");
        let mut buf = [0u8; 6];
        assert_eq!(r.read(&mut buf).expect("read"), 6);
        assert_eq!(&buf, b"hello\0", "NUL 分隔符原样过管道");
        // 先把剩余排空，EOF 才轮得到
        let mut rest = [0u8; 5];
        assert_eq!(r.read(&mut rest).expect("read rest"), 5);
        assert_eq!(&rest, b"world");
        drop(w);
        assert_eq!(r.read(&mut buf).expect("read eof"), 0, "写端关即 EOF");
    }
}
