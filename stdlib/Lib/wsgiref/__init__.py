"""wsgiref — WSGI Utilities and Reference Implementation"""

from wsgiref.headers import Headers
from wsgiref.util import setup_testing_defaults, request_uri
from wsgiref.simple_server import make_server, demo_app, WSGIServer, WSGIRequestHandler
from wsgiref.handlers import BaseHandler, SimpleHandler, BaseCGIHandler, CGIHandler
from wsgiref.validate import validator

__all__ = [
    'Headers', 'setup_testing_defaults', 'request_uri',
    'make_server', 'demo_app', 'WSGIServer', 'WSGIRequestHandler',
    'BaseHandler', 'SimpleHandler', 'BaseCGIHandler', 'CGIHandler',
    'validator',
]
