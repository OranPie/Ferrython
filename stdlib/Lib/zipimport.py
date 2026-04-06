"""zipimport — Import modules from zip archives.

Stub implementation for Ferrython. Provides the zipimporter class
interface but does not actually support importing from zip files.
"""

__all__ = ["zipimporter", "ZipImportError"]


class ZipImportError(ImportError):
    """Exception raised by zipimporter objects."""
    pass


class zipimporter:
    """zipimporter(archivepath) -> zipimporter object

    Create a new zipimporter instance. 'archivepath' must be a path to
    a zipfile, or a specific path inside a zipfile.
    """

    def __init__(self, path):
        self.archive = path
        self._files = {}

    def find_module(self, fullname, path=None):
        """find_module(fullname, path=None) -> self or None.

        Search for a module specified by 'fullname'. 'fullname' must be
        the fully qualified (dotted) module name.
        """
        return None

    def find_loader(self, fullname, path=None):
        """find_loader(fullname, path=None) -> self or None.

        Search for a module specified by 'fullname'.
        """
        return None, []

    def load_module(self, fullname):
        """load_module(fullname) -> module.

        Load the module specified by 'fullname'.
        """
        raise ZipImportError(
            "can't find module '{}'".format(fullname)
        )

    def get_filename(self, fullname):
        """get_filename(fullname) -> filename string."""
        raise ZipImportError(
            "can't find module '{}'".format(fullname)
        )

    def is_package(self, fullname):
        """is_package(fullname) -> bool."""
        raise ZipImportError(
            "can't find module '{}'".format(fullname)
        )

    def get_data(self, pathname):
        """get_data(pathname) -> string with file data."""
        raise ZipImportError(
            "can't find file '{}'".format(pathname)
        )

    def get_code(self, fullname):
        """get_code(fullname) -> code object."""
        raise ZipImportError(
            "can't find module '{}'".format(fullname)
        )

    def get_source(self, fullname):
        """get_source(fullname) -> source string."""
        raise ZipImportError(
            "can't find module '{}'".format(fullname)
        )

    def __repr__(self):
        return "<zipimporter object '{}'>".format(self.archive)
