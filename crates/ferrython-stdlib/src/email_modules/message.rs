use super::*;

// ── email.message module ───────────────────────────────────────────────

fn email_message_constructor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let _ = args;
    Ok(build_message_instance(None, None))
}

pub fn create_email_message_module() -> PyObjectRef {
    make_module(
        "email.message",
        vec![
            ("Message", make_builtin(email_message_constructor)),
            ("EmailMessage", make_builtin(email_message_constructor)),
        ],
    )
}
