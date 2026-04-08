"""
distutils.errors — Exception classes for distutils.
"""

class DistutilsError(Exception):
    pass

class DistutilsModuleError(DistutilsError):
    pass

class DistutilsFileError(DistutilsError):
    pass

class DistutilsOptionError(DistutilsError):
    pass

class DistutilsSetupError(DistutilsError):
    pass

class DistutilsPlatformError(DistutilsError):
    pass

class DistutilsExecError(DistutilsError):
    pass

class DistutilsInternalError(DistutilsError):
    pass

class DistutilsTemplateError(DistutilsError):
    pass

class DistutilsByteCompileError(DistutilsError):
    pass

class CCompilerError(Exception):
    pass

class CompileError(CCompilerError):
    pass

class LibError(CCompilerError):
    pass

class LinkError(CCompilerError):
    pass

class UnknownFileError(CCompilerError):
    pass
