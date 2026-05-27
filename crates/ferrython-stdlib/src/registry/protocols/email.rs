use crate::email_modules;
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "email" => Some(email_modules::create_email_module()),
        "email.errors" => Some(email_modules::create_email_errors_module()),
        "email.message" => Some(email_modules::create_email_message_module()),
        "email.mime" => Some(email_modules::create_email_mime_module()),
        "email.mime.text" => Some(email_modules::create_email_mime_text_module()),
        "email.mime.multipart" => Some(email_modules::create_email_mime_multipart_module()),
        "email.mime.base" => Some(email_modules::create_email_mime_base_module()),
        "email.mime.application" => Some(email_modules::create_email_mime_application_module()),
        "email.mime.image" => Some(email_modules::create_email_mime_image_module()),
        "email.utils" => Some(email_modules::create_email_utils_module()),
        "email.policy" => Some(email_modules::create_email_policy_module()),
        "email.contentmanager" => Some(email_modules::create_email_contentmanager_module()),
        "email.charset" => Some(email_modules::create_email_charset_module()),
        _ => None,
    }
}
