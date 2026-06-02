//! Miscellaneous stdlib modules

mod builtins;
mod cmd;
mod compileall;
#[allow(dead_code)]
mod contextlib;
mod contextvars;
#[allow(dead_code)]
mod copy_module;
mod ctypes;
mod curses;
mod dataclasses;
mod future;
mod getopt;
mod graphlib;
mod keyword;
mod lowrisk;
mod mimetypes;
mod netrc;
mod plistlib;
mod pstats;
mod quopri;
mod readline;
mod runpy;
mod stringprep;
mod tomllib;
mod webbrowser;

pub use builtins::create_builtins_module;
pub use cmd::create_cmd_module;
pub use compileall::create_compileall_module;
pub use contextvars::create_contextvars_module;
pub use ctypes::create_ctypes_module;
pub use curses::create_curses_module;
pub use dataclasses::create_dataclasses_module;
pub use future::create_future_module;
pub use getopt::create_getopt_module;
pub use graphlib::create_graphlib_module;
pub use keyword::create_keyword_module;
pub use lowrisk::{
    create_chunk_module, create_filecmp_module, create_imghdr_module, create_nturl2path_module,
    create_sndhdr_module, create_uu_module, create_xdrlib_module,
};
pub use mimetypes::create_mimetypes_module;
pub use netrc::create_netrc_module;
pub use plistlib::create_plistlib_module;
pub use pstats::create_pstats_module;
pub use quopri::create_quopri_module;
pub use readline::create_readline_module;
pub use runpy::create_runpy_module;
pub use stringprep::create_stringprep_module;
pub use tomllib::create_tomllib_module;
pub use webbrowser::create_webbrowser_module;
