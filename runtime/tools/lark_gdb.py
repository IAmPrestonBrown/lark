"""Value formatters for a Lark program, under gdb.

Rule X-3 already emits `#line`, so gdb shows Lark source and Lark line numbers
with no help. What it cannot show is a managed value: a `gc Person*` prints as
an address.

This script reads the object header that rule M-4 puts before every payload,
and the field map that rule M-5 puts in the descriptor. It needs no metadata
that the compiler does not already emit.

Load it with:

    source build/lark_gdb.py

The build writes a copy into the output directory, so the path is beside the
program.
"""

import gdb


def header_type():
    """Returns the `lark_header` type, or None outside a Lark program."""
    try:
        return gdb.lookup_type("lark_header")
    except gdb.error:
        return None


def header_of(pointer):
    """Returns the header that sits before a managed payload.

    Rule M-4 puts it at a negative offset, so the payload keeps C layout and
    rule O-3 holds.
    """
    kind = header_type()
    if kind is None or int(pointer) == 0:
        return None
    address = int(pointer) - kind.sizeof
    try:
        return gdb.Value(address).cast(kind.pointer()).dereference()
    except gdb.error:
        return None


class ManagedPrinter:
    """Prints a managed pointer as its type, its count, and its address."""

    def __init__(self, value):
        self.value = value

    def to_string(self):
        address = int(self.value)
        if address == 0:
            return "null"

        header = header_of(self.value)
        if header is None:
            return "0x%x (no header)" % address
        try:
            info = header["type"]
            if int(info) == 0:
                return "0x%x (no header)" % address
            name = info["name"].string()
            count = int(header["count"])
        except gdb.error:
            return "0x%x (no header)" % address

        if count == 1:
            return "%s at 0x%x" % (name, address)
        return "%s[%d] at 0x%x" % (name, count, address)


def lookup(value):
    """Chooses a printer for one value."""
    kind = value.type.strip_typedefs()
    if kind.code != gdb.TYPE_CODE_PTR:
        return None
    if header_type() is None:
        return None
    return ManagedPrinter(value)


class GcStats(gdb.Command):
    """Prints the collector statistics. Use `gc-stats` at the prompt."""

    def __init__(self):
        super(GcStats, self).__init__("gc-stats", gdb.COMMAND_DATA)

    def invoke(self, _argument, _from_tty):
        try:
            stats = gdb.parse_and_eval("lark_gc_statistics()")
            name = gdb.parse_and_eval("lark_gc_name()")
        except gdb.error:
            print("the program does not link the Lark runtime")
            return
        print("collector      %s" % name.string())
        for field in (
            "live_objects",
            "live_bytes",
            "total_allocations",
            "collections",
            "heap_bytes",
        ):
            print("%-14s %s" % (field, stats[field]))


gdb.pretty_printers.append(lookup)
GcStats()
print("lark: managed values print their type. `gc-stats` prints the heap.")
