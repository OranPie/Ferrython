use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{
    check_args, make_builtin, make_module, PyObject, PyObjectMethods, PyObjectRef,
};

const S_IFDIR: i64 = 0o040000;
const S_IFCHR: i64 = 0o020000;
const S_IFBLK: i64 = 0o060000;
const S_IFREG: i64 = 0o100000;
const S_IFIFO: i64 = 0o010000;
const S_IFLNK: i64 = 0o120000;
const S_IFSOCK: i64 = 0o140000;
const S_IFMT: i64 = 0o170000;

const S_ISUID: i64 = 0o4000;
const S_ISGID: i64 = 0o2000;
const S_ISVTX: i64 = 0o1000;

const S_IRWXU: i64 = 0o0700;
const S_IRUSR: i64 = 0o0400;
const S_IWUSR: i64 = 0o0200;
const S_IXUSR: i64 = 0o0100;

const S_IRWXG: i64 = 0o0070;
const S_IRGRP: i64 = 0o0040;
const S_IWGRP: i64 = 0o0020;
const S_IXGRP: i64 = 0o0010;

const S_IRWXO: i64 = 0o0007;
const S_IROTH: i64 = 0o0004;
const S_IWOTH: i64 = 0o0002;
const S_IXOTH: i64 = 0o0001;

pub fn create_stat_module() -> PyObjectRef {
    make_module(
        "stat",
        vec![
            ("S_IFDIR", PyObject::int(S_IFDIR)),
            ("S_IFCHR", PyObject::int(S_IFCHR)),
            ("S_IFBLK", PyObject::int(S_IFBLK)),
            ("S_IFREG", PyObject::int(S_IFREG)),
            ("S_IFIFO", PyObject::int(S_IFIFO)),
            ("S_IFLNK", PyObject::int(S_IFLNK)),
            ("S_IFSOCK", PyObject::int(S_IFSOCK)),
            ("S_IFMT", PyObject::int(S_IFMT)),
            ("S_ISUID", PyObject::int(S_ISUID)),
            ("S_ISGID", PyObject::int(S_ISGID)),
            ("S_ISVTX", PyObject::int(S_ISVTX)),
            ("S_IRWXU", PyObject::int(S_IRWXU)),
            ("S_IRUSR", PyObject::int(S_IRUSR)),
            ("S_IWUSR", PyObject::int(S_IWUSR)),
            ("S_IXUSR", PyObject::int(S_IXUSR)),
            ("S_IRWXG", PyObject::int(S_IRWXG)),
            ("S_IRGRP", PyObject::int(S_IRGRP)),
            ("S_IWGRP", PyObject::int(S_IWGRP)),
            ("S_IXGRP", PyObject::int(S_IXGRP)),
            ("S_IRWXO", PyObject::int(S_IRWXO)),
            ("S_IROTH", PyObject::int(S_IROTH)),
            ("S_IWOTH", PyObject::int(S_IWOTH)),
            ("S_IXOTH", PyObject::int(S_IXOTH)),
            ("S_ENFMT", PyObject::int(S_ISGID)),
            ("S_IREAD", PyObject::int(S_IRUSR)),
            ("S_IWRITE", PyObject::int(S_IWUSR)),
            ("S_IEXEC", PyObject::int(S_IXUSR)),
            ("S_ISDIR", make_builtin(|args| mode_is(args, S_IFDIR))),
            ("S_ISCHR", make_builtin(|args| mode_is(args, S_IFCHR))),
            ("S_ISBLK", make_builtin(|args| mode_is(args, S_IFBLK))),
            ("S_ISREG", make_builtin(|args| mode_is(args, S_IFREG))),
            ("S_ISFIFO", make_builtin(|args| mode_is(args, S_IFIFO))),
            ("S_ISLNK", make_builtin(|args| mode_is(args, S_IFLNK))),
            ("S_ISSOCK", make_builtin(|args| mode_is(args, S_IFSOCK))),
            ("S_IMODE", make_builtin(stat_imode)),
            ("S_IFMT_func", make_builtin(stat_ifmt)),
            ("filemode", make_builtin(stat_filemode)),
            ("FILE_ATTRIBUTE_ARCHIVE", PyObject::int(32)),
            ("FILE_ATTRIBUTE_COMPRESSED", PyObject::int(2048)),
            ("FILE_ATTRIBUTE_DEVICE", PyObject::int(64)),
            ("FILE_ATTRIBUTE_DIRECTORY", PyObject::int(16)),
            ("FILE_ATTRIBUTE_ENCRYPTED", PyObject::int(16384)),
            ("FILE_ATTRIBUTE_HIDDEN", PyObject::int(2)),
            ("FILE_ATTRIBUTE_INTEGRITY_STREAM", PyObject::int(32768)),
            ("FILE_ATTRIBUTE_NORMAL", PyObject::int(128)),
            ("FILE_ATTRIBUTE_NOT_CONTENT_INDEXED", PyObject::int(8192)),
            ("FILE_ATTRIBUTE_NO_SCRUB_DATA", PyObject::int(131072)),
            ("FILE_ATTRIBUTE_OFFLINE", PyObject::int(4096)),
            ("FILE_ATTRIBUTE_READONLY", PyObject::int(1)),
            ("FILE_ATTRIBUTE_REPARSE_POINT", PyObject::int(1024)),
            ("FILE_ATTRIBUTE_SPARSE_FILE", PyObject::int(512)),
            ("FILE_ATTRIBUTE_SYSTEM", PyObject::int(4)),
            ("FILE_ATTRIBUTE_TEMPORARY", PyObject::int(256)),
            ("FILE_ATTRIBUTE_VIRTUAL", PyObject::int(65536)),
        ],
    )
}

fn mode_arg(name: &str, args: &[PyObjectRef]) -> PyResult<i64> {
    check_args(name, args, 1)?;
    args[0].to_int()
}

fn mode_is(args: &[PyObjectRef], kind: i64) -> PyResult<PyObjectRef> {
    let mode = mode_arg("stat mode test", args)?;
    Ok(PyObject::bool_val((mode & S_IFMT) == kind))
}

fn stat_imode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let mode = mode_arg("stat.S_IMODE", args)?;
    Ok(PyObject::int(mode & 0o7777))
}

fn stat_ifmt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let mode = mode_arg("stat.S_IFMT_func", args)?;
    Ok(PyObject::int(mode & S_IFMT))
}

fn stat_filemode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let mode = mode_arg("stat.filemode", args)?;
    let mut out = String::with_capacity(10);
    out.push(file_type_char(mode));
    out.push(mode_char(mode, S_IRUSR, 'r', '-'));
    out.push(mode_char(mode, S_IWUSR, 'w', '-'));
    out.push(exec_char(mode, S_IXUSR, S_ISUID, 's', 'S'));
    out.push(mode_char(mode, S_IRGRP, 'r', '-'));
    out.push(mode_char(mode, S_IWGRP, 'w', '-'));
    out.push(exec_char(mode, S_IXGRP, S_ISGID, 's', 'S'));
    out.push(mode_char(mode, S_IROTH, 'r', '-'));
    out.push(mode_char(mode, S_IWOTH, 'w', '-'));
    out.push(exec_char(mode, S_IXOTH, S_ISVTX, 't', 'T'));
    Ok(PyObject::str_val(CompactString::from(out)))
}

fn file_type_char(mode: i64) -> char {
    match mode & S_IFMT {
        S_IFLNK => 'l',
        S_IFREG => '-',
        S_IFBLK => 'b',
        S_IFDIR => 'd',
        S_IFCHR => 'c',
        S_IFIFO => 'p',
        S_IFSOCK => 's',
        _ => '-',
    }
}

fn mode_char(mode: i64, bit: i64, set: char, unset: char) -> char {
    if mode & bit == bit {
        set
    } else {
        unset
    }
}

fn exec_char(mode: i64, exec_bit: i64, special_bit: i64, both: char, special_only: char) -> char {
    match (
        mode & exec_bit == exec_bit,
        mode & special_bit == special_bit,
    ) {
        (true, true) => both,
        (false, true) => special_only,
        (true, false) => 'x',
        (false, false) => '-',
    }
}
