"""Value formatters for a Lark program, under lldb.

Rule X-3 already emits `#line`, so lldb shows Lark source and Lark line
numbers with no help. What it cannot show is a managed value: a `gc Person*`
prints as an address.

This script reads the object header that rule M-4 puts before every payload,
and the field map that rule M-5 puts in the descriptor. It needs no metadata
that the compiler does not already emit.

Load it with:

    command script import build/lark_lldb.py

The build writes a copy into the output directory, so the path is beside the
program.
"""

import lldb


def header_of(value):
    """Returns the `lark_header` that sits before a managed payload.

    Rule M-4 puts the header at a negative offset, so a managed pointer keeps
    C layout and rule O-3 holds.
    """
    target = value.GetTarget()
    header_type = target.FindFirstType("lark_header")
    if not header_type.IsValid():
        return None
    address = value.GetValueAsUnsigned(0)
    if address == 0:
        return None
    start = address - header_type.GetByteSize()
    return value.CreateValueFromAddress(
        "header", start, header_type
    )


def text_of(value):
    """Returns the text that a `const char *` points at.

    The string is read from memory rather than taken from the summary,
    because this script also formats a pointer and the two would then
    depend on each other.
    """
    if value is None or not value.IsValid():
        return None
    address = value.GetValueAsUnsigned(0)
    if address == 0:
        return None
    process = value.GetProcess()
    if not process.IsValid():
        return None
    error = lldb.SBError()
    text = process.ReadCStringFromMemory(address, 256, error)
    if error.Fail() or not text:
        return None
    return text


def type_name_of(header):
    """Returns the name that the descriptor records."""
    if header is None or not header.IsValid():
        return None
    info = header.GetChildMemberWithName("type")
    if not info.IsValid() or info.GetValueAsUnsigned(0) == 0:
        return None
    return text_of(info.Dereference().GetChildMemberWithName("name"))


def managed_summary(value, _internal):
    """Prints a managed pointer as its type, its count, and its address.

    A pointer that no header sits behind prints as a plain address. The
    formatter runs on every pointer, and most of them are ordinary.
    """
    address = value.GetValueAsUnsigned(0)
    if address == 0:
        return "null"

    # A text pointer keeps the summary that lldb already gives it.
    name = value.GetType().GetName()
    if "char" in name:
        return None

    header = header_of(value)
    name = type_name_of(header)
    if name is None:
        return "0x%x" % address

    count = header.GetChildMemberWithName("count").GetValueAsUnsigned(1)
    if count == 1:
        return "%s at 0x%x" % (name, address)
    return "%s[%d] at 0x%x" % (name, count, address)


def gc_stats(debugger, command, result, _internal):
    """Prints the collector statistics. Use `gc-stats` at the prompt."""
    frame = (
        debugger.GetSelectedTarget()
        .GetProcess()
        .GetSelectedThread()
        .GetSelectedFrame()
    )
    stats = frame.EvaluateExpression("lark_gc_statistics()")
    if not stats.IsValid() or stats.GetError().Fail():
        result.SetError("the program does not link the Lark runtime")
        return
    name = frame.EvaluateExpression("lark_gc_name()")
    result.AppendMessage("collector          %s" % (text_of(name) or "unknown"))
    for field in (
        "live_objects",
        "live_bytes",
        "total_allocations",
        "collections",
        "heap_bytes",
    ):
        member = stats.GetChildMemberWithName(field)
        result.AppendMessage(
            "%-18s %s" % (field, member.GetValueAsUnsigned(0))
        )


def __lldb_init_module(debugger, _internal):
    """Registers the formatter and the command."""
    # The formatter runs on every pointer. A `char *` and any other pointer
    # with no header behind it falls through to the plain address, so the
    # match needs no cleverness of its own.
    debugger.HandleCommand(
        "type summary add --python-function "
        "lark_lldb.managed_summary --regex '.+ [*]+$'"
    )
    debugger.HandleCommand(
        "command script add -f lark_lldb.gc_stats gc-stats"
    )
    print("lark: managed values print their type. `gc-stats` prints the heap.")
