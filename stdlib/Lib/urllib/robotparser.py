"""robotparser - Parser for robots.txt files"""

import urllib.parse


class RobotFileParser:
    """Parse a robots.txt file."""

    def __init__(self, url=''):
        self.entries = []
        self.sitemaps = []
        self.default_entry = None
        self.disallow_all = False
        self.allow_all = False
        self.set_url(url)
        self.last_checked = 0

    def mtime(self):
        return self.last_checked

    def modified(self):
        import time
        self.last_checked = time.time()

    def set_url(self, url):
        self.url = url

    def read(self):
        """Read the robots.txt URL and feed it to the parser."""
        pass

    def _add_entry(self, entry):
        if "*" in entry.useragents:
            if self.default_entry is None:
                self.default_entry = entry
        else:
            self.entries.append(entry)

    def parse(self, lines):
        """Parse the input lines from a robots.txt file."""
        self.entries = []
        self.sitemaps = []
        self.default_entry = None
        self.disallow_all = False
        self.allow_all = False

        entry = Entry()
        state = 0  # 0=start, 1=saw-agent, 2=saw-rule

        for line in lines:
            if isinstance(line, bytes):
                line = line.decode('utf-8', errors='replace')
            # Remove comments
            i = line.find('#')
            if i >= 0:
                line = line[:i]
            line = line.strip()
            if not line:
                if state == 2:
                    self._add_entry(entry)
                    entry = Entry()
                    state = 0
                continue

            parts = line.split(':', 1)
            if len(parts) != 2:
                continue
            field = parts[0].strip().lower()
            value = parts[1].strip()

            if field == 'user-agent':
                if state == 2:
                    self._add_entry(entry)
                    entry = Entry()
                entry.useragents.append(value.lower())
                state = 1
            elif field == 'disallow':
                if state != 0:
                    entry.rulelines.append(RuleLine(value, False))
                    state = 2
            elif field == 'allow':
                if state != 0:
                    entry.rulelines.append(RuleLine(value, True))
                    state = 2
            elif field == 'crawl-delay':
                if state != 0:
                    try:
                        entry.delay = float(value)
                    except ValueError:
                        pass
                    state = 2
            elif field == 'sitemap':
                self.sitemaps.append(value)

        if state == 2:
            self._add_entry(entry)

    def can_fetch(self, useragent, url):
        """Return True if the useragent is allowed to fetch the url."""
        if self.disallow_all:
            return False
        if self.allow_all:
            return True

        parsed = urllib.parse.urlparse(url)
        path = urllib.parse.quote(parsed.path) if parsed.path else '/'
        if parsed.query:
            path = path + '?' + parsed.query

        ua = useragent.lower()
        for entry in self.entries:
            if entry.applies_to(ua):
                return entry.allowance(path)

        if self.default_entry:
            return self.default_entry.allowance(path)
        return True

    def crawl_delay(self, useragent):
        ua = useragent.lower()
        for entry in self.entries:
            if entry.applies_to(ua):
                return entry.delay
        if self.default_entry:
            return self.default_entry.delay
        return None

    def site_maps(self):
        if not self.sitemaps:
            return None
        return self.sitemaps

    def __str__(self):
        entries = self.entries[:]
        if self.default_entry is not None:
            entries.append(self.default_entry)
        return '\n'.join(str(e) for e in entries)


class RuleLine:
    def __init__(self, path, allowance):
        if path == '' and not allowance:
            allowance = True
        path = urllib.parse.quote(path) if path else '/'
        self.path = path
        self.allowance = allowance

    def applies_to(self, filename):
        return filename.startswith(self.path)

    def __str__(self):
        return ("Allow" if self.allowance else "Disallow") + ": " + self.path


class Entry:
    def __init__(self):
        self.useragents = []
        self.rulelines = []
        self.delay = None

    def applies_to(self, useragent):
        ua = useragent.lower()
        for agent in self.useragents:
            if agent == '*' or ua in agent or agent in ua:
                return True
        return False

    def allowance(self, filename):
        for line in self.rulelines:
            if line.applies_to(filename):
                return line.allowance
        return True

    def __str__(self):
        lines = []
        for agent in self.useragents:
            lines.append("User-agent: " + agent)
        for rule in self.rulelines:
            lines.append(str(rule))
        return '\n'.join(lines)
