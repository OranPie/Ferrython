"""IP address manipulation library.

Simplified implementation of the standard library ipaddress module,
compatible with CPython 3.8's ipaddress interface for IPv4.
"""


class AddressValueError(ValueError):
    """A Value Error related to the address."""
    pass


class NetmaskValueError(ValueError):
    """A Value Error related to the netmask."""
    pass


class IPv4Address:
    """Represent and manipulate single IPv4 addresses."""

    _max_prefixlen = 32

    def __init__(self, address):
        if isinstance(address, int):
            if address < 0 or address > 0xFFFFFFFF:
                raise AddressValueError(
                    f"{address} is not a valid IPv4 address (out of range)")
            self._ip = address
        elif isinstance(address, bytes):
            if len(address) != 4:
                raise AddressValueError(
                    f"Expected 4 bytes, got {len(address)}")
            self._ip = (address[0] << 24) | (address[1] << 16) | \
                       (address[2] << 8) | address[3]
        elif isinstance(address, str):
            self._ip = self._parse_string(address)
        elif isinstance(address, IPv4Address):
            self._ip = address._ip
        else:
            raise TypeError(
                f"Cannot interpret {type(address).__name__} as IPv4 address")

    def _parse_string(self, addr_str):
        addr_str = addr_str.strip()
        parts = addr_str.split('.')
        if len(parts) != 4:
            raise AddressValueError(
                f"{addr_str!r}: Expected 4 octets, got {len(parts)}")
        result = 0
        for i, part in enumerate(parts):
            if not part:
                raise AddressValueError(
                    f"Empty octet in {addr_str!r}")
            if len(part) > 1 and part[0] == '0':
                raise AddressValueError(
                    f"Leading zeros in {addr_str!r}")
            try:
                val = int(part)
            except ValueError:
                raise AddressValueError(
                    f"Invalid octet {part!r} in {addr_str!r}")
            if val < 0 or val > 255:
                raise AddressValueError(
                    f"Octet {val} not in range 0..255 in {addr_str!r}")
            result = (result << 8) | val
        return result

    @property
    def packed(self):
        return bytes([
            (self._ip >> 24) & 0xFF,
            (self._ip >> 16) & 0xFF,
            (self._ip >> 8) & 0xFF,
            self._ip & 0xFF
        ])

    @property
    def is_private(self):
        # 10.0.0.0/8
        if (self._ip >> 24) == 10:
            return True
        # 172.16.0.0/12
        if (self._ip >> 20) == (172 << 4 | 1):  # 0xAC1
            return True
        # 192.168.0.0/16
        if (self._ip >> 16) == (192 << 8 | 168):  # 0xC0A8
            return True
        return False

    @property
    def is_loopback(self):
        return (self._ip >> 24) == 127

    @property
    def is_multicast(self):
        first = (self._ip >> 24) & 0xFF
        return 224 <= first <= 239

    @property
    def is_reserved(self):
        first = (self._ip >> 24) & 0xFF
        return 240 <= first <= 255

    @property
    def is_unspecified(self):
        return self._ip == 0

    @property
    def is_link_local(self):
        return (self._ip >> 16) == (169 << 8 | 254)  # 169.254.0.0/16

    @property
    def is_global(self):
        return not (self.is_private or self.is_loopback or
                    self.is_link_local or self.is_multicast or
                    self.is_reserved or self.is_unspecified)

    @property
    def version(self):
        return 4

    @property
    def max_prefixlen(self):
        return 32

    def __str__(self):
        return "{}.{}.{}.{}".format(
            (self._ip >> 24) & 0xFF,
            (self._ip >> 16) & 0xFF,
            (self._ip >> 8) & 0xFF,
            self._ip & 0xFF
        )

    def __repr__(self):
        return "IPv4Address('" + str(self) + "')"

    def __int__(self):
        return self._ip

    def __eq__(self, other):
        if isinstance(other, IPv4Address):
            return self._ip == other._ip
        return NotImplemented

    def __ne__(self, other):
        if isinstance(other, IPv4Address):
            return self._ip != other._ip
        return NotImplemented

    def __lt__(self, other):
        if isinstance(other, IPv4Address):
            return self._ip < other._ip
        return NotImplemented

    def __le__(self, other):
        if isinstance(other, IPv4Address):
            return self._ip <= other._ip
        return NotImplemented

    def __gt__(self, other):
        if isinstance(other, IPv4Address):
            return self._ip > other._ip
        return NotImplemented

    def __ge__(self, other):
        if isinstance(other, IPv4Address):
            return self._ip >= other._ip
        return NotImplemented

    def __hash__(self):
        return hash(self._ip)

    def __add__(self, other):
        if isinstance(other, int):
            return IPv4Address(self._ip + other)
        return NotImplemented

    def __sub__(self, other):
        if isinstance(other, int):
            return IPv4Address(self._ip - other)
        elif isinstance(other, IPv4Address):
            return self._ip - other._ip
        return NotImplemented


class IPv4Network:
    """Represent and manipulate IPv4 networks."""

    def __init__(self, address, strict=True):
        if isinstance(address, str):
            if '/' in address:
                addr_str, prefix_str = address.split('/', 1)
                self._prefixlen = int(prefix_str)
                if self._prefixlen < 0 or self._prefixlen > 32:
                    raise NetmaskValueError(
                        f"Invalid prefix length: {self._prefixlen}")
                addr = IPv4Address(addr_str)
            else:
                addr = IPv4Address(address)
                self._prefixlen = 32
        elif isinstance(address, IPv4Network):
            addr = address.network_address
            self._prefixlen = address.prefixlen
        else:
            raise TypeError(
                f"Cannot interpret {type(address).__name__} as network")

        mask = self._prefix_to_mask(self._prefixlen)
        net_int = int(addr) & mask
        if strict and net_int != int(addr):
            raise ValueError(
                "{} has host bits set".format(address))
        self._network_address = IPv4Address(net_int)
        self._broadcast_address = IPv4Address(net_int | (~mask & 0xFFFFFFFF))

    def _prefix_to_mask(self, prefixlen):
        if prefixlen == 0:
            return 0
        return (0xFFFFFFFF << (32 - prefixlen)) & 0xFFFFFFFF

    @property
    def network_address(self):
        return self._network_address

    @property
    def broadcast_address(self):
        return self._broadcast_address

    @property
    def prefixlen(self):
        return self._prefixlen

    @property
    def netmask(self):
        return IPv4Address(self._prefix_to_mask(self._prefixlen))

    @property
    def hostmask(self):
        mask = self._prefix_to_mask(self._prefixlen)
        return IPv4Address(~mask & 0xFFFFFFFF)

    @property
    def num_addresses(self):
        return 1 << (32 - self._prefixlen)

    @property
    def version(self):
        return 4

    @property
    def is_private(self):
        return self._network_address.is_private

    @property
    def is_loopback(self):
        return self._network_address.is_loopback

    def __contains__(self, addr):
        if isinstance(addr, IPv4Address):
            mask = self._prefix_to_mask(self._prefixlen)
            return (int(addr) & mask) == int(self._network_address)
        return False

    def __str__(self):
        return str(self._network_address) + "/" + str(self._prefixlen)

    def __repr__(self):
        return "IPv4Network('" + str(self) + "')"

    def __eq__(self, other):
        if isinstance(other, IPv4Network):
            return (int(self._network_address) == int(other._network_address)
                    and self._prefixlen == other._prefixlen)
        return NotImplemented

    def __hash__(self):
        return hash((int(self._network_address), self._prefixlen))

    def overlaps(self, other):
        if not isinstance(other, IPv4Network):
            raise TypeError("expected IPv4Network")
        return (self._network_address in other or
                other._network_address in self)

    def subnet_of(self, other):
        if not isinstance(other, IPv4Network):
            raise TypeError("expected IPv4Network")
        if self._prefixlen < other._prefixlen:
            return False
        return self._network_address in other

    def supernet_of(self, other):
        return other.subnet_of(self)

    def subnets(self, prefixlen_diff=1):
        new_prefixlen = self._prefixlen + prefixlen_diff
        if new_prefixlen > 32:
            raise ValueError("prefix length diff too large")
        step = 1 << (32 - new_prefixlen)
        start = int(self._network_address)
        end = int(self._broadcast_address) + 1
        result = []
        current = start
        while current < end:
            net = IPv4Network(str(IPv4Address(current)) + "/" + str(new_prefixlen))
            result.append(net)
            current += step
        return result


class IPv4Interface(IPv4Address):
    """Represent an IPv4 host address with network info."""

    def __init__(self, address):
        if isinstance(address, str) and '/' in address:
            addr_str, prefix_str = address.split('/', 1)
            super().__init__(addr_str)
            self._prefixlen = int(prefix_str)
        else:
            super().__init__(address)
            self._prefixlen = 32

    @property
    def network(self):
        mask = (0xFFFFFFFF << (32 - self._prefixlen)) & 0xFFFFFFFF
        net_addr = self._ip & mask
        return IPv4Network(str(IPv4Address(net_addr)) + "/" + str(self._prefixlen))

    def __str__(self):
        return super().__str__() + "/" + str(self._prefixlen)

    def __repr__(self):
        return "IPv4Interface('" + str(self) + "')"


def ip_address(address):
    """Create an IPv4Address from the given value."""
    if isinstance(address, str) and ':' in address:
        raise NotImplementedError("IPv6 not yet supported")
    return IPv4Address(address)


def ip_network(address, strict=True):
    """Create an IPv4Network from the given value."""
    if isinstance(address, str) and ':' in address:
        raise NotImplementedError("IPv6 not yet supported")
    return IPv4Network(address, strict)


def ip_interface(address):
    """Create an IPv4Interface from the given value."""
    if isinstance(address, str) and ':' in address:
        raise NotImplementedError("IPv6 not yet supported")
    return IPv4Interface(address)
