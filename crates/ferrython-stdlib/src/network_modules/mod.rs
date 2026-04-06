//! Network stdlib modules: socket, urllib, http, ssl.

mod socket_module;
mod http_module;

pub use socket_module::create_socket_module;
pub use http_module::{
    create_urllib_module, create_urllib_parse_module, create_http_module,
    create_http_server_module, create_http_cookiejar_module, create_ssl_module,
    create_smtplib_module, create_ftplib_module, create_imaplib_module,
    create_poplib_module, create_cgi_module, create_xmlrpc_module,
};
