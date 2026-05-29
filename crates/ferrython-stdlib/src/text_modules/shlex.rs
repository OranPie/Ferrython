use super::*;
use ferrython_core::object::call_callable;

const DEFAULT_PUNCTUATION_CHARS: &str = "();<>|&";

#[derive(Clone)]
struct ShlexConfig {
    posix: bool,
    comments: bool,
    punctuation_chars: String,
    whitespace_split: bool,
}

pub fn create_shlex_module() -> PyObjectRef {
    make_module(
        "shlex",
        vec![
            ("split", make_builtin(shlex_split)),
            ("quote", make_builtin(shlex_quote)),
            ("join", make_builtin(shlex_join)),
            ("shlex", make_builtin(shlex_ctor)),
        ],
    )
}

fn shlex_split(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("shlex.split requires 1 argument"));
    }
    let kwargs = trailing_kwargs(args);
    let comments = kw_bool(kwargs, "comments", false);
    let posix = kw_bool(kwargs, "posix", true);
    let config = ShlexConfig {
        posix,
        comments,
        punctuation_chars: String::new(),
        whitespace_split: true,
    };
    let tokens = tokenize(&args[0].py_to_string(), &config);
    Ok(PyObject::list(
        tokens
            .into_iter()
            .map(|token| PyObject::str_val(CompactString::from(token)))
            .collect(),
    ))
}

fn shlex_quote(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("shlex.quote requires 1 argument"));
    }
    Ok(PyObject::str_val(CompactString::from(shell_quote(
        &args[0].py_to_string(),
    ))))
}

fn shlex_join(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("shlex.join requires 1 argument"));
    }
    let items = args[0].to_list()?;
    let parts: Vec<String> = items
        .iter()
        .map(|item| shell_quote(&item.py_to_string()))
        .collect();
    Ok(PyObject::str_val(CompactString::from(parts.join(" "))))
}

fn shlex_ctor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let kwargs = trailing_kwargs(args);
    let positional_len = args.len() - usize::from(kwargs.is_some());
    let instream = args
        .first()
        .filter(|_| positional_len > 0)
        .cloned()
        .unwrap_or_else(|| PyObject::str_val(CompactString::from("")));
    let posix = if positional_len > 1 {
        args[1].is_truthy()
    } else {
        kw_bool(kwargs, "posix", false)
    };
    let punctuation_chars = kw_punctuation(kwargs);
    let source = input_to_string(&instream)?;
    let config = ShlexConfig {
        posix,
        comments: true,
        punctuation_chars: punctuation_chars.clone(),
        whitespace_split: false,
    };

    let source = Rc::new(source);
    let base_config = Rc::new(config);
    let token_cache = Rc::new(RefCell::new(None::<Vec<String>>));
    let index = Rc::new(RefCell::new(0usize));
    let pushed = Rc::new(RefCell::new(Vec::<String>::new()));
    let token_text = Rc::new(RefCell::new(String::new()));

    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__setattr__"),
        PyObject::native_closure("shlex.__setattr__", move |call_args| {
            if call_args.len() < 3 {
                return Err(PyException::type_error("__setattr__ requires 3 arguments"));
            }
            let name = call_args[1].py_to_string();
            if name == "punctuation_chars" {
                return Err(PyException::attribute_error(
                    "property 'punctuation_chars' of 'shlex' object has no setter",
                ));
            }
            if let PyObjectPayload::Instance(inst) = &call_args[0].payload {
                inst.attrs
                    .write()
                    .insert(CompactString::from(name), call_args[2].clone());
            }
            Ok(PyObject::none())
        }),
    );
    let cls = PyObject::class(CompactString::from("shlex"), vec![], ns);
    let inst = PyObject::instance(cls);

    if let PyObjectPayload::Instance(data) = &inst.payload {
        let mut attrs = data.attrs.write();
        attrs.insert(CompactString::from("eof"), PyObject::none());
        attrs.insert(
            CompactString::from("whitespace"),
            PyObject::str_val(CompactString::from(" \t\r\n")),
        );
        attrs.insert(
            CompactString::from("quotes"),
            PyObject::str_val(CompactString::from("'\"")),
        );
        attrs.insert(
            CompactString::from("escape"),
            PyObject::str_val(CompactString::from("\\")),
        );
        attrs.insert(
            CompactString::from("commenters"),
            PyObject::str_val(CompactString::from("#")),
        );
        attrs.insert(
            CompactString::from("wordchars"),
            PyObject::str_val(CompactString::from(wordchars_for(&punctuation_chars))),
        );
        attrs.insert(
            CompactString::from("punctuation_chars"),
            PyObject::str_val(CompactString::from(punctuation_chars.as_str())),
        );
        attrs.insert(CompactString::from("posix"), PyObject::bool_val(posix));
        attrs.insert(
            CompactString::from("whitespace_split"),
            PyObject::bool_val(false),
        );
        attrs.insert(
            CompactString::from("token"),
            PyObject::str_val(CompactString::from("")),
        );

        attrs.insert(CompactString::from("__iter__"), {
            let self_obj = inst.clone();
            PyObject::native_closure("shlex.__iter__", move |_| Ok(self_obj.clone()))
        });
        attrs.insert(CompactString::from("__next__"), {
            let self_obj = inst.clone();
            let source_ref = source.clone();
            let config_ref = base_config.clone();
            let cache_ref = token_cache.clone();
            let index_ref = index.clone();
            let pushed_ref = pushed.clone();
            let token_ref = token_text.clone();
            PyObject::native_closure("shlex.__next__", move |_| {
                match next_token(
                    &self_obj,
                    &source_ref,
                    &config_ref,
                    &cache_ref,
                    &index_ref,
                    &pushed_ref,
                    &token_ref,
                )? {
                    Some(token) => Ok(PyObject::str_val(CompactString::from(token))),
                    None => Err(PyException::stop_iteration()),
                }
            })
        });
        attrs.insert(CompactString::from("get_token"), {
            let self_obj = inst.clone();
            let source_ref = source.clone();
            let config_ref = base_config.clone();
            let cache_ref = token_cache.clone();
            let index_ref = index.clone();
            let pushed_ref = pushed.clone();
            let token_ref = token_text.clone();
            PyObject::native_closure("shlex.get_token", move |_| {
                match next_token(
                    &self_obj,
                    &source_ref,
                    &config_ref,
                    &cache_ref,
                    &index_ref,
                    &pushed_ref,
                    &token_ref,
                )? {
                    Some(token) => Ok(PyObject::str_val(CompactString::from(token))),
                    None => Ok(PyObject::none()),
                }
            })
        });
        attrs.insert(CompactString::from("push_token"), {
            let pushed_ref = pushed.clone();
            PyObject::native_closure("shlex.push_token", move |call_args| {
                check_args_min("push_token", call_args, 1)?;
                pushed_ref.borrow_mut().push(call_args[0].py_to_string());
                Ok(PyObject::none())
            })
        });
    }

    Ok(inst)
}

fn next_token(
    owner: &PyObjectRef,
    source: &str,
    base_config: &ShlexConfig,
    token_cache: &Rc<RefCell<Option<Vec<String>>>>,
    index: &Rc<RefCell<usize>>,
    pushed: &Rc<RefCell<Vec<String>>>,
    token_text: &Rc<RefCell<String>>,
) -> PyResult<Option<String>> {
    if let Some(token) = pushed.borrow_mut().pop() {
        set_token_attr(owner, &token);
        *token_text.borrow_mut() = token.clone();
        return Ok(Some(token));
    }
    if token_cache.borrow().is_none() {
        let mut config = base_config.clone();
        config.whitespace_split = owner
            .get_attr("whitespace_split")
            .map(|value| value.is_truthy())
            .unwrap_or(config.whitespace_split);
        config.comments = owner
            .get_attr("commenters")
            .map(|value| !value.py_to_string().is_empty())
            .unwrap_or(config.comments);
        *token_cache.borrow_mut() = Some(tokenize(source, &config));
    }
    let mut idx = index.borrow_mut();
    let cache = token_cache.borrow();
    let tokens = cache.as_ref().unwrap();
    if *idx >= tokens.len() {
        set_token_attr(owner, "");
        token_text.borrow_mut().clear();
        return Ok(None);
    }
    let token = tokens[*idx].clone();
    *idx += 1;
    set_token_attr(owner, &token);
    *token_text.borrow_mut() = token.clone();
    Ok(Some(token))
}

fn set_token_attr(owner: &PyObjectRef, token: &str) {
    if let PyObjectPayload::Instance(inst) = &owner.payload {
        inst.attrs.write().insert(
            CompactString::from("token"),
            PyObject::str_val(CompactString::from(token)),
        );
    }
}

fn trailing_kwargs(args: &[PyObjectRef]) -> Option<&PyObjectRef> {
    args.last()
        .filter(|arg| matches!(&arg.payload, PyObjectPayload::Dict(_)))
}

fn kw_bool(kwargs: Option<&PyObjectRef>, name: &str, default: bool) -> bool {
    let Some(kwargs) = kwargs else {
        return default;
    };
    if let PyObjectPayload::Dict(map) = &kwargs.payload {
        if let Some(value) = map
            .read()
            .get(&HashableKey::str_key(CompactString::from(name)))
            .cloned()
        {
            return value.is_truthy();
        }
    }
    default
}

fn kw_punctuation(kwargs: Option<&PyObjectRef>) -> String {
    let Some(kwargs) = kwargs else {
        return String::new();
    };
    if let PyObjectPayload::Dict(map) = &kwargs.payload {
        if let Some(value) = map
            .read()
            .get(&HashableKey::str_key(CompactString::from(
                "punctuation_chars",
            )))
            .cloned()
        {
            return match &value.payload {
                PyObjectPayload::Bool(true) => DEFAULT_PUNCTUATION_CHARS.to_string(),
                PyObjectPayload::Bool(false) | PyObjectPayload::None => String::new(),
                _ => value.py_to_string(),
            };
        }
    }
    String::new()
}

fn input_to_string(obj: &PyObjectRef) -> PyResult<String> {
    if let Some(read_fn) = obj.get_attr("read") {
        let value = call_callable(&read_fn, &[])?;
        return Ok(value.py_to_string());
    }
    if matches!(&obj.payload, PyObjectPayload::None) {
        Ok(String::new())
    } else {
        Ok(obj.py_to_string())
    }
}

fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    let safe_chars = "@%_-+=:,./";
    if s.chars()
        .all(|ch| ch.is_ascii_alphanumeric() || safe_chars.contains(ch))
    {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

fn wordchars_for(punctuation_chars: &str) -> String {
    let mut chars = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_".to_string();
    for ch in punctuation_chars.chars() {
        chars.retain(|candidate| candidate != ch);
    }
    chars
}

fn tokenize(input: &str, config: &ShlexConfig) -> Vec<String> {
    let chars: Vec<char> = input.chars().collect();
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut i = 0usize;
    let mut quoted = false;
    while i < chars.len() {
        let ch = chars[i];
        if config.comments && ch == '#' {
            if config.posix && !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
                quoted = false;
            }
            i += 1;
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            if i < chars.len() && chars[i] == '\n' {
                i += 1;
            }
            continue;
        }
        if ch.is_whitespace() {
            if !current.is_empty() || (config.posix && quoted) {
                tokens.push(std::mem::take(&mut current));
                quoted = false;
            }
            i += 1;
            continue;
        }
        if is_punctuation(ch, config)
            || (config.posix
                && !config.whitespace_split
                && config.punctuation_chars.is_empty()
                && DEFAULT_PUNCTUATION_CHARS.contains(ch))
        {
            if !current.is_empty() || (config.posix && quoted) {
                tokens.push(std::mem::take(&mut current));
                quoted = false;
            }
            let mut punct = String::new();
            punct.push(ch);
            i += 1;
            if config.punctuation_chars == DEFAULT_PUNCTUATION_CHARS {
                while i < chars.len() && is_punctuation(chars[i], config) {
                    punct.push(chars[i]);
                    i += 1;
                }
            } else if config.punctuation_chars.contains(ch) {
                while i < chars.len() && chars[i] == ch {
                    punct.push(chars[i]);
                    i += 1;
                }
            }
            tokens.push(punct);
            continue;
        }
        if ch == '\'' || ch == '"' {
            if !config.posix && !current.is_empty() {
                current.push(ch);
                i += 1;
                continue;
            }
            let quote = ch;
            if !config.posix {
                current.push(ch);
            }
            quoted = true;
            i += 1;
            while i < chars.len() {
                let inner = chars[i];
                if inner == quote {
                    if !config.posix {
                        current.push(inner);
                    }
                    i += 1;
                    break;
                }
                if config.posix && quote == '"' && inner == '\\' {
                    i += 1;
                    if i < chars.len() {
                        if "\\\"$`\n".contains(chars[i]) {
                            current.push(chars[i]);
                        } else {
                            current.push('\\');
                            current.push(chars[i]);
                        }
                        i += 1;
                    }
                } else {
                    current.push(inner);
                    i += 1;
                }
            }
            if !config.posix && !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
                quoted = false;
            }
            continue;
        }
        if config.posix && ch == '\\' {
            i += 1;
            if i < chars.len() {
                current.push(chars[i]);
                i += 1;
            }
            continue;
        }
        if !config.posix && is_split_nonposix_char(ch, config) {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            tokens.push(ch.to_string());
            i += 1;
            continue;
        }
        current.push(ch);
        i += 1;
    }
    if !current.is_empty() || (config.posix && quoted) {
        tokens.push(current);
    }
    tokens
}

fn is_punctuation(ch: char, config: &ShlexConfig) -> bool {
    !config.punctuation_chars.is_empty() && config.punctuation_chars.contains(ch)
}

fn is_split_nonposix_char(ch: char, config: &ShlexConfig) -> bool {
    let safe = if config.punctuation_chars.is_empty() {
        "@%+=:,./~*?"
    } else {
        "@%+=:,./-~*?"
    };
    !config.whitespace_split && !ch.is_ascii_alphanumeric() && ch != '_' && !safe.contains(ch)
}
