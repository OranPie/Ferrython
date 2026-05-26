//! Text processing stdlib modules (string, re, textwrap, fnmatch)

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::helpers::register_bytearray_export;
use ferrython_core::object::{
    check_args_min, make_builtin, make_module, new_fx_hashkey_map, FxHashKeyMap, IteratorData,
    PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::OnceLock;

use super::fs_modules::glob_match;

mod difflib;
mod encodings;
mod pprint;
mod regex_impl;
pub use difflib::create_difflib_module;
pub use pprint::create_pprint_module;
pub use regex_impl::{create_re_module, create_sre_module};
mod fnmatch;
mod html;
mod html_parser;
mod unicodedata;
pub use html_parser::create_html_parser_module;
pub use unicodedata::create_unicodedata_module;
use unicodedata::unicode_lookup_name;
mod shlex;
mod string;
pub use string::create_string_module;
mod textwrap;
pub use encodings::{
    create_encodings_aliases_module, create_encodings_codec_module, create_encodings_idna_module,
    create_encodings_module, create_multibytecodec_module, create_string_internal_module,
};
pub use fnmatch::create_fnmatch_module;
pub use html::create_html_module;
pub use shlex::create_shlex_module;
pub use textwrap::create_textwrap_module;
