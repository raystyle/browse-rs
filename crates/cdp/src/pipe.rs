//! 匿名管道薄封装。std 的 anonymous pipe（`std::io::pipe`）至 1.98 仍未稳定，
//! Windows 直接用 `CreatePipe`；POSIX 留桩（管道通道暂仅 Windows，走端口态）。
//!
//! 句柄存 `usize` 以获得 `Send + Sync`；阻塞 IO 由 [`crate::Session`] 的
//! 管道泵在 blocking 线程池上跑。

use anyhow::{Result, anyhow};

/// 管道读端（启动器持有，读 chrome 写出的 CDP）。
pub struct PipeReader {
    /// OS 句柄（Windows HANDLE）。
    pub h: usize,
}

/// 管道写端（启动器持有，往 chrome 写 CDP）。
pub struct PipeWriter {
    /// OS 句柄（Windows HANDLE）。
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

/// POSIX 桩：管道通道未实现（S005 的 fd 3/4 布线），spawn 走端口态。
///
/// # Errors
///
/// 恒定错误（POSIX 未实现）。
#[cfg(not(windows))]
pub fn anon_pair() -> Result<(PipeReader, PipeWriter)> {
    Err(anyhow!("POSIX 管道通道未实现；spawn 走默认端口态"))
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

#[cfg(not(windows))]
pub fn set_inheritable(_h: usize, _on: bool) -> Result<()> {
    Ok(())
}
