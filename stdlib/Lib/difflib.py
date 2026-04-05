"""difflib module — Helpers for computing deltas."""

class SequenceMatcher:
    """Flexible class for comparing pairs of sequences."""
    
    def __init__(self, isjunk=None, a='', b='', autojunk=True):
        self.isjunk = isjunk
        self.a = a
        self.b = b
        self.autojunk = autojunk
        self._matching_blocks = None
        self._opcodes = None
    
    def set_seqs(self, a, b):
        self.set_seq1(a)
        self.set_seq2(b)
    
    def set_seq1(self, a):
        self.a = a
        self._matching_blocks = None
        self._opcodes = None
    
    def set_seq2(self, b):
        self.b = b
        self._matching_blocks = None
        self._opcodes = None
    
    def find_longest_match(self, alo=0, ahi=None, blo=0, bhi=None):
        """Find longest matching block in a[alo:ahi] and b[blo:bhi]."""
        a, b = self.a, self.b
        if ahi is None:
            ahi = len(a)
        if bhi is None:
            bhi = len(b)
        
        besti, bestj, bestsize = alo, blo, 0
        j2len = {}
        
        for i in range(alo, ahi):
            newj2len = {}
            for j in range(blo, bhi):
                if a[i] == b[j]:
                    k = newj2len[j] = j2len.get(j - 1, 0) + 1
                    if k > bestsize:
                        besti, bestj, bestsize = i - k + 1, j - k + 1, k
            j2len = newj2len
        
        return Match(besti, bestj, bestsize)
    
    def get_matching_blocks(self):
        """Return list of triples describing matching subsequences."""
        if self._matching_blocks is not None:
            return self._matching_blocks
        
        la, lb = len(self.a), len(self.b)
        queue = [(0, la, 0, lb)]
        matching_blocks = []
        
        while queue:
            alo, ahi, blo, bhi = queue.pop()
            m = self.find_longest_match(alo, ahi, blo, bhi)
            i, j, k = m.a, m.b, m.size
            if k:
                matching_blocks.append(m)
                if alo < i and blo < j:
                    queue.append((alo, i, blo, j))
                if i + k < ahi and j + k < bhi:
                    queue.append((i + k, ahi, j + k, bhi))
        
        matching_blocks.sort()
        # Collapse adjacent equal blocks
        i1 = j1 = k1 = 0
        non_adjacent = []
        for m in matching_blocks:
            i2, j2, k2 = m.a, m.b, m.size
            if i1 + k1 == i2 and j1 + k1 == j2:
                k1 += k2
            else:
                if k1:
                    non_adjacent.append(Match(i1, j1, k1))
                i1, j1, k1 = i2, j2, k2
        if k1:
            non_adjacent.append(Match(i1, j1, k1))
        
        non_adjacent.append(Match(la, lb, 0))
        self._matching_blocks = non_adjacent
        return self._matching_blocks
    
    def ratio(self):
        """Return a measure of the sequences' similarity (float in [0,1])."""
        matches = sum(triple.size for triple in self.get_matching_blocks())
        length = len(self.a) + len(self.b)
        if length:
            return 2.0 * matches / length
        return 1.0
    
    def quick_ratio(self):
        """Return an upper bound on ratio() relatively quickly."""
        return self.ratio()
    
    def real_quick_ratio(self):
        """Return an upper bound on ratio() very quickly."""
        la, lb = len(self.a), len(self.b)
        return 2.0 * min(la, lb) / (la + lb) if (la + lb) else 1.0
    
    def get_opcodes(self):
        """Return 5-tuples describing how to turn a into b."""
        if self._opcodes is not None:
            return self._opcodes
        i = j = 0
        self._opcodes = answer = []
        for m in self.get_matching_blocks():
            ai, bj, size = m.a, m.b, m.size
            tag = ''
            if i < ai and j < bj:
                tag = 'replace'
            elif i < ai:
                tag = 'delete'
            elif j < bj:
                tag = 'insert'
            if tag:
                answer.append((tag, i, ai, j, bj))
            i, j = ai + size, bj + size
            if size:
                answer.append(('equal', ai, i, bj, j))
        return answer


class Match:
    """Named-tuple-like for match results."""
    __slots__ = ('a', 'b', 'size')
    
    def __init__(self, a, b, size):
        self.a = a
        self.b = b
        self.size = size
    
    def __repr__(self):
        return "Match(a={}, b={}, size={})".format(self.a, self.b, self.size)
    
    def __iter__(self):
        yield self.a
        yield self.b
        yield self.size
    
    def __lt__(self, other):
        if self.a != other.a:
            return self.a < other.a
        if self.b != other.b:
            return self.b < other.b
        return self.size < other.size
    
    def __eq__(self, other):
        return self.a == other.a and self.b == other.b and self.size == other.size


def get_close_matches(word, possibilities, n=3, cutoff=0.6):
    """Use SequenceMatcher to return list of the best matches."""
    result = []
    s = SequenceMatcher()
    s.set_seq2(word)
    for x in possibilities:
        s.set_seq1(x)
        if s.ratio() >= cutoff:
            result.append((s.ratio(), x))
    result.sort(reverse=True)
    return [x for score, x in result[:n]]


def unified_diff(a, b, fromfile='', tofile='', fromfiledate='', tofiledate='',
                 n=3, lineterm='\n'):
    """Compare two sequences of lines and generate the delta as a unified diff."""
    started = False
    for group in SequenceMatcher(None, a, b).get_opcodes():
        tag, i1, i2, j1, j2 = group
        if tag == 'equal':
            continue
        if not started:
            started = True
            yield '--- {}{}'.format(fromfile, lineterm)
            yield '+++ {}{}'.format(tofile, lineterm)
        if tag in ('replace', 'delete'):
            for line in a[i1:i2]:
                yield '-' + line
        if tag in ('replace', 'insert'):
            for line in b[j1:j2]:
                yield '+' + line


def context_diff(a, b, fromfile='', tofile='', fromfiledate='', tofiledate='',
                 n=3, lineterm='\n'):
    """Compare two sequences of lines; generate the delta as a context diff."""
    prefix = {'insert': '+ ', 'delete': '- ', 'replace': '! ', 'equal': '  '}
    started = False
    for group in SequenceMatcher(None, a, b).get_opcodes():
        tag, i1, i2, j1, j2 = group
        if tag == 'equal':
            continue
        if not started:
            started = True
            yield '*** {}{}'.format(fromfile, lineterm)
            yield '--- {}{}'.format(tofile, lineterm)
        for line in a[i1:i2]:
            yield prefix.get(tag, '  ') + line
        for line in b[j1:j2]:
            yield prefix.get(tag, '  ') + line


def ndiff(a, b, linejunk=None, charjunk=None):
    """Compare two sequences of lines; generate the delta as ndiff output."""
    for group in SequenceMatcher(None, a, b).get_opcodes():
        tag, i1, i2, j1, j2 = group
        if tag == 'equal':
            for line in a[i1:i2]:
                yield '  ' + line
        elif tag == 'replace':
            for line in a[i1:i2]:
                yield '- ' + line
            for line in b[j1:j2]:
                yield '+ ' + line
        elif tag == 'delete':
            for line in a[i1:i2]:
                yield '- ' + line
        elif tag == 'insert':
            for line in b[j1:j2]:
                yield '+ ' + line
