use super::*;

mod environ;
mod fd;
mod fs_ops;
mod misc;
mod pathlike;
mod permissions;
mod process;
mod stat;
mod system_info;
mod terminal;
mod walk;

use environ::create_environ_object;
use fd::{
    os_close, os_dup, os_dup2, os_fdopen, os_fstat, os_fsync, os_ftruncate, os_link, os_lseek,
    os_open, os_pipe, os_read, os_truncate, os_write,
};
use fs_ops::{
    os_chdir, os_getcwd, os_listdir, os_makedirs, os_mkdir, os_remove, os_removedirs, os_rename,
    os_replace, os_rmdir,
};
use misc::{
    os_access, os_expanduser, os_fsdecode, os_fsencode, os_getlogin, os_putenv, os_strerror,
    os_umask, os_unsetenv, os_urandom,
};
use pathlike::{create_pathlike_class, os_fspath};
use permissions::{os_chmod, os_chown, os_isatty, os_readlink, os_symlink};
use process::{
    os_cpu_count, os_getegid, os_getenv, os_geteuid, os_getgid, os_getpid, os_getppid, os_getuid,
    os_kill, os_popen, os_system, os_waitpid, os_wexitstatus, os_wifexited, os_wifsignaled,
    os_wifstopped, os_wstopsig, os_wtermsig,
};
use stat::{make_stat_result_class, os_lstat, os_scandir, os_stat};
use system_info::{os_get_terminal_size, os_times, os_uname};
use terminal::make_terminal_size_class;
pub use terminal::make_terminal_size_instance;
use walk::os_walk;

// ── os module ──

pub fn create_os_module() -> PyObjectRef {
    make_module(
        "os",
        vec![
            (
                "name",
                PyObject::str_val(CompactString::from(if cfg!(windows) {
                    "nt"
                } else {
                    "posix"
                })),
            ),
            (
                "sep",
                PyObject::str_val(CompactString::from(std::path::MAIN_SEPARATOR.to_string())),
            ),
            (
                "linesep",
                PyObject::str_val(CompactString::from(if cfg!(windows) {
                    "\r\n"
                } else {
                    "\n"
                })),
            ),
            ("curdir", PyObject::str_val(CompactString::from("."))),
            ("pardir", PyObject::str_val(CompactString::from(".."))),
            ("extsep", PyObject::str_val(CompactString::from("."))),
            ("getcwd", make_builtin(os_getcwd)),
            ("listdir", make_builtin(os_listdir)),
            ("mkdir", make_builtin(os_mkdir)),
            ("makedirs", make_builtin(os_makedirs)),
            ("remove", make_builtin(os_remove)),
            ("unlink", make_builtin(os_remove)),
            ("rmdir", make_builtin(os_rmdir)),
            ("removedirs", make_builtin(os_removedirs)),
            ("rename", make_builtin(os_rename)),
            ("replace", make_builtin(os_replace)),
            ("path", create_os_path_module()),
            ("getenv", make_builtin(os_getenv)),
            ("environ", create_environ_object()),
            (
                "_Environ",
                PyObject::class(CompactString::from("_Environ"), vec![], IndexMap::new()),
            ),
            ("cpu_count", make_builtin(os_cpu_count)),
            ("getpid", make_builtin(os_getpid)),
            ("fspath", PyObject::native_function("os.fspath", os_fspath)),
            ("PathLike", create_pathlike_class()),
            ("walk", make_builtin(os_walk)),
            ("stat", make_builtin(os_stat)),
            ("chmod", make_builtin(os_chmod)),
            ("chown", make_builtin(os_chown)),
            ("symlink", make_builtin(os_symlink)),
            ("readlink", make_builtin(os_readlink)),
            ("isatty", make_builtin(os_isatty)),
            ("chdir", make_builtin(os_chdir)),
            ("system", make_builtin(os_system)),
            ("popen", make_builtin(os_popen)),
            ("getppid", make_builtin(os_getppid)),
            ("urandom", make_builtin(os_urandom)),
            ("access", make_builtin(os_access)),
            ("umask", make_builtin(os_umask)),
            ("getlogin", make_builtin(os_getlogin)),
            (
                "devnull",
                PyObject::str_val(CompactString::from(if cfg!(windows) {
                    "nul"
                } else {
                    "/dev/null"
                })),
            ),
            ("F_OK", PyObject::int(0)),
            ("R_OK", PyObject::int(4)),
            ("W_OK", PyObject::int(2)),
            ("X_OK", PyObject::int(1)),
            ("O_RDONLY", PyObject::int(0)),
            ("O_WRONLY", PyObject::int(1)),
            ("O_RDWR", PyObject::int(2)),
            ("O_CREAT", PyObject::int(0o100)),
            ("O_EXCL", PyObject::int(0o200)),
            ("O_NOCTTY", PyObject::int(0o400)),
            ("O_TRUNC", PyObject::int(0o1000)),
            ("O_APPEND", PyObject::int(0o2000)),
            ("O_NONBLOCK", PyObject::int(0o4000)),
            ("O_CLOEXEC", PyObject::int(0o2000000)),
            ("SEEK_SET", PyObject::int(0)),
            ("SEEK_CUR", PyObject::int(1)),
            ("SEEK_END", PyObject::int(2)),
            ("strerror", make_builtin(os_strerror)),
            ("scandir", make_builtin(os_scandir)),
            ("putenv", make_builtin(os_putenv)),
            ("unsetenv", make_builtin(os_unsetenv)),
            ("lstat", make_builtin(os_lstat)),
            ("expanduser", make_builtin(os_expanduser)),
            // Unix ID functions
            ("getuid", make_builtin(os_getuid)),
            ("getgid", make_builtin(os_getgid)),
            ("geteuid", make_builtin(os_geteuid)),
            ("getegid", make_builtin(os_getegid)),
            ("getppid", make_builtin(os_getppid)),
            // Process management
            ("kill", make_builtin(os_kill)),
            // File operations
            ("link", make_builtin(os_link)),
            ("truncate", make_builtin(os_truncate)),
            // Pipe and fd operations
            ("pipe", make_builtin(os_pipe)),
            ("dup", make_builtin(os_dup)),
            ("dup2", make_builtin(os_dup2)),
            // terminal_size class exposed on the os module
            ("terminal_size", make_terminal_size_class()),
            // Terminal/system info
            ("get_terminal_size", make_builtin(os_get_terminal_size)),
            ("uname", make_builtin(os_uname)),
            ("times", make_builtin(os_times)),
            // Path constants
            (
                "pathsep",
                PyObject::str_val(CompactString::from(if cfg!(windows) { ";" } else { ":" })),
            ),
            ("altsep", PyObject::none()),
            // Low-level file descriptor operations
            ("close", make_builtin(os_close)),
            ("open", make_builtin(os_open)),
            ("read", make_builtin(os_read)),
            ("write", make_builtin(os_write)),
            ("fdopen", make_builtin(os_fdopen)),
            ("fstat", make_builtin(os_fstat)),
            ("ftruncate", make_builtin(os_ftruncate)),
            ("lseek", make_builtin(os_lseek)),
            ("fsync", make_builtin(os_fsync)),
            ("stat_result", make_builtin(make_stat_result_class)),
            // waitpid and W* macros
            ("waitpid", make_builtin(os_waitpid)),
            ("WNOHANG", PyObject::int(1)),
            ("WUNTRACED", PyObject::int(2)),
            ("WIFEXITED", make_builtin(os_wifexited)),
            ("WEXITSTATUS", make_builtin(os_wexitstatus)),
            ("WIFSIGNALED", make_builtin(os_wifsignaled)),
            ("WTERMSIG", make_builtin(os_wtermsig)),
            ("WIFSTOPPED", make_builtin(os_wifstopped)),
            ("WSTOPSIG", make_builtin(os_wstopsig)),
            ("fsencode", make_builtin(os_fsencode)),
            ("fsdecode", make_builtin(os_fsdecode)),
        ],
    )
}
