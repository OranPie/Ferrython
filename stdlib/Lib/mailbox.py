"""mailbox - Read/write support for Maildir, mbox, MH, Babyl, MMDF mailboxes."""

import os
import io
import email
import email.message


class Error(Exception):
    pass

class NoSuchMailboxError(Error):
    pass

class NotEmptyError(Error):
    pass

class ExternalClashError(Error):
    pass

class FormatError(Error):
    pass


class Mailbox:
    """A group of messages in a particular place."""

    def __init__(self, path, factory=None, create=True):
        self._path = os.path.abspath(os.path.expanduser(path))
        self._factory = factory

    def add(self, message):
        raise NotImplementedError('Method not implemented')

    def remove(self, key):
        raise NotImplementedError('Method not implemented')

    def __delitem__(self, key):
        self.remove(key)

    def discard(self, key):
        try:
            self.remove(key)
        except KeyError:
            pass

    def __setitem__(self, key, message):
        raise NotImplementedError('Method not implemented')

    def get(self, key, default=None):
        try:
            return self.__getitem__(key)
        except KeyError:
            return default

    def __getitem__(self, key):
        raise NotImplementedError('Method not implemented')

    def get_message(self, key):
        raise NotImplementedError('Method not implemented')

    def get_string(self, key):
        raise NotImplementedError('Method not implemented')

    def get_bytes(self, key):
        raise NotImplementedError('Method not implemented')

    def get_file(self, key):
        raise NotImplementedError('Method not implemented')

    def iterkeys(self):
        raise NotImplementedError('Method not implemented')

    def keys(self):
        return list(self.iterkeys())

    def itervalues(self):
        for key in self.iterkeys():
            try:
                yield self[key]
            except KeyError:
                continue

    def __iter__(self):
        return self.itervalues()

    def values(self):
        return list(self.itervalues())

    def iteritems(self):
        for key in self.iterkeys():
            try:
                yield (key, self[key])
            except KeyError:
                continue

    def items(self):
        return list(self.iteritems())

    def __contains__(self, key):
        raise NotImplementedError('Method not implemented')

    def __len__(self):
        raise NotImplementedError('Method not implemented')

    def clear(self):
        for key in self.keys():
            self.discard(key)

    def pop(self, key, *args):
        try:
            result = self[key]
        except KeyError:
            if args:
                return args[0]
            raise
        self.discard(key)
        return result

    def popitem(self):
        for key in self.iterkeys():
            return (key, self.pop(key))
        raise KeyError('mailbox is empty')

    def update(self, arg=None):
        if hasattr(arg, 'iteritems'):
            source = arg.iteritems()
        elif hasattr(arg, 'items'):
            source = arg.items()
        else:
            source = arg
        bad_key = False
        for key, message in source:
            try:
                self[key] = message
            except KeyError:
                bad_key = True
        if bad_key:
            raise KeyError('No message with key(s)')

    def flush(self):
        raise NotImplementedError('Method not implemented')

    def lock(self):
        raise NotImplementedError('Method not implemented')

    def unlock(self):
        raise NotImplementedError('Method not implemented')

    def close(self):
        raise NotImplementedError('Method not implemented')

    def _dump_message(self, message, target, mangle_from_=False):
        if isinstance(message, email.message.Message):
            data = message.as_string()
        elif isinstance(message, str):
            data = message
        elif isinstance(message, bytes):
            data = message.decode('ascii', errors='replace')
        elif hasattr(message, 'read'):
            data = message.read()
        else:
            raise TypeError('Invalid message type: %s' % type(message))
        if mangle_from_:
            data = data.replace('\nFrom ', '\n>From ')
        if isinstance(target, io.IOBase) or hasattr(target, 'write'):
            target.write(data)
        else:
            raise TypeError('target must have a write method')


class mbox(Mailbox):
    """A classic mbox mailbox."""

    _mangle_from_ = True

    def __init__(self, path, factory=None, create=True):
        super().__init__(path, factory, create)
        self._messages = {}
        self._next_key = 0
        self._file = None

    def add(self, message):
        key = self._next_key
        self._next_key += 1
        if isinstance(message, str):
            self._messages[key] = message
        elif isinstance(message, email.message.Message):
            self._messages[key] = message.as_string()
        else:
            self._messages[key] = str(message)
        return key

    def remove(self, key):
        if key not in self._messages:
            raise KeyError(key)
        del self._messages[key]

    def __setitem__(self, key, message):
        if isinstance(message, str):
            self._messages[key] = message
        elif isinstance(message, email.message.Message):
            self._messages[key] = message.as_string()
        else:
            self._messages[key] = str(message)

    def __getitem__(self, key):
        if key not in self._messages:
            raise KeyError(key)
        return self._messages[key]

    def iterkeys(self):
        return iter(sorted(self._messages.keys()))

    def __contains__(self, key):
        return key in self._messages

    def __len__(self):
        return len(self._messages)

    def flush(self):
        pass

    def lock(self):
        pass

    def unlock(self):
        pass

    def close(self):
        self.flush()


class Maildir(Mailbox):
    """A Maildir mailbox."""

    def __init__(self, dirname, factory=None, create=True):
        super().__init__(dirname, factory, create)
        self._messages = {}
        self._next_key = 0

    def add(self, message):
        key = str(self._next_key)
        self._next_key += 1
        if isinstance(message, str):
            self._messages[key] = message
        elif isinstance(message, email.message.Message):
            self._messages[key] = message.as_string()
        else:
            self._messages[key] = str(message)
        return key

    def remove(self, key):
        if key not in self._messages:
            raise KeyError(key)
        del self._messages[key]

    def __setitem__(self, key, message):
        if isinstance(message, str):
            self._messages[key] = message
        else:
            self._messages[key] = str(message)

    def __getitem__(self, key):
        if key not in self._messages:
            raise KeyError(key)
        return self._messages[key]

    def iterkeys(self):
        return iter(sorted(self._messages.keys()))

    def __contains__(self, key):
        return key in self._messages

    def __len__(self):
        return len(self._messages)

    def flush(self):
        pass

    def lock(self):
        pass

    def unlock(self):
        pass

    def close(self):
        self.flush()


class Message(email.message.Message):
    """Message with mailbox-format-specific properties."""

    def __init__(self, message=None):
        if message is None:
            super().__init__()
        elif isinstance(message, email.message.Message):
            super().__init__()
            # Copy headers
        elif isinstance(message, str):
            super().__init__()
        else:
            super().__init__()


class mboxMessage(Message):
    _from = ''

    def get_from(self):
        return self._from

    def set_from(self, from_, time_=None):
        self._from = from_


class MaildirMessage(Message):
    _subdir = 'new'
    _info = ''

    def get_subdir(self):
        return self._subdir

    def set_subdir(self, subdir):
        if subdir not in ('new', 'cur'):
            raise ValueError("subdir must be 'new' or 'cur'")
        self._subdir = subdir

    def get_info(self):
        return self._info

    def set_info(self, info):
        self._info = info
