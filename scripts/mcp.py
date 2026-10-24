#!/usr/bin/env python3
"""Talks to the Slint MCP server of a running RustCut (debug builds have the default `slint-mcp` feature; the app listens
when started with SLINT_MCP_PORT, as `rc-testenv session` does). Standard library only.

    scripts/mcp.py [--port P] tools                     list the server's tools
    scripts/mcp.py [--port P] call TOOL [JSON]          call a tool, print its result
    scripts/mcp.py [--port P] tree [MAX]                element tree of the main window (type, id, label, geometry)
    scripts/mcp.py [--port P] find ID                   handles of the elements with a qualified id (MainWindow::player)
    scripts/mcp.py [--port P] click ID                  left click on the first element with that id
    scripts/mcp.py [--port P] click-nth ID N [double]   left (or double) click on the N-th element with that id (0 = first)
    scripts/mcp.py [--port P] click-type TYPE N         left click on the N-th element of a component type (`RatingButton`)
    scripts/mcp.py [--port P] move X Y                  the pointer to window position X, Y (hover; no button)
    scripts/mcp.py [--port P] shot FILE                 PNG screenshot of the main window (as Slint renders it)
    scripts/mcp.py [--port P] key CHORD                 a key or chord (`ctrl+s`, `Escape`, `shift+Right`) delivered
                                                        inside the app: nothing reaches other windows, nothing stays held
    scripts/mcp.py [--port P] type TEXT                 text typed inside the app, one character after the other

`--window N` (after `--port`) acts on the N-th open window instead of the first (the Settings window once it is open).
The port defaults to $SLINT_MCP_PORT, then 9315. Exit code 1 on any error.
"""

import base64
import json
import os
import sys
import urllib.request

DEFAULT_PORT = 9315


def rpc(port, method, params=None, ident=1):
    body = json.dumps({"jsonrpc": "2.0", "id": ident, "method": method, "params": params or {}}).encode()
    request = urllib.request.Request(
        f"http://127.0.0.1:{port}/mcp", data=body, headers={"Content-Type": "application/json"}
    )
    with urllib.request.urlopen(request, timeout=20) as response:
        answer = json.loads(response.read() or b"{}")
    if "error" in answer:
        raise RuntimeError(f"{method}: {answer['error']}")
    return answer.get("result", {})


def call(port, tool, arguments=None):
    """The tool's content blocks; text blocks holding JSON are decoded."""
    result = rpc(port, "tools/call", {"name": tool, "arguments": arguments or {}})
    if result.get("isError"):
        raise RuntimeError(f"{tool}: {result.get('content')}")
    blocks = []
    for block in result.get("content", []):
        if block.get("type") == "text":
            try:
                blocks.append(json.loads(block["text"]))
            except (ValueError, KeyError):
                blocks.append(block.get("text"))
        else:
            blocks.append(block)
    return blocks


def first(blocks):
    return blocks[0] if blocks else None


WINDOW = 0


def main_window(port):
    windows = first(call(port, "list_windows"))
    handles = windows.get("windowHandles") if isinstance(windows, dict) else windows
    if not handles or len(handles) <= WINDOW:
        raise RuntimeError(f"no window {WINDOW}: {windows}")
    return handles[WINDOW]


# Slint's key texts (`Key` namespace) for the names a chord may use.
KEY_TEXT = {
    "ctrl": "\u0011", "control": "\u0011", "shift": "\u0010", "alt": "\u0012", "meta": "\u0017",
    "escape": "\u001b", "esc": "\u001b", "return": "\n", "enter": "\n", "tab": "\t", "backspace": "\u0008",
    "delete": "\u007f", "space": " ", "up": "\uf700", "down": "\uf701", "left": "\uf702", "right": "\uf703",
    "home": "\uf729", "end": "\uf72b", "pageup": "\uf72c", "pagedown": "\uf72d", "comma": ",", "period": ".",
    "plus": "+", "minus": "-",
}
MODIFIERS = ("ctrl", "control", "shift", "alt", "meta")


def key_text(name):
    lower = name.lower()
    if lower in KEY_TEXT:
        return KEY_TEXT[lower]
    if lower.startswith("f") and lower[1:].isdigit():
        return chr(0xF704 + int(lower[1:]) - 1)
    if len(name) == 1:
        return name.lower()
    raise ValueError(f"unknown key {name}")


def key(port, chord):
    """Presses the modifiers, types the key, and releases the modifiers again (also when a step fails)."""
    window = main_window(port)
    parts = chord.split("+")
    mods, last = [p for p in parts[:-1] if p.lower() in MODIFIERS], parts[-1]
    pressed = []
    try:
        for mod in mods:
            call(port, "dispatch_key_event", {"windowHandle": window, "text": key_text(mod), "eventType": "Press"})
            pressed.append(mod)
        call(port, "dispatch_key_event", {"windowHandle": window, "text": key_text(last), "eventType": "PressAndRelease"})
    finally:
        for mod in reversed(pressed):
            try:
                call(port, "dispatch_key_event", {"windowHandle": window, "text": key_text(mod), "eventType": "Release"})
            except (OSError, RuntimeError):
                pass  # the window is gone (the key quit the app): nothing is held anywhere


def type_text(port, text):
    window = main_window(port)
    for char in text:
        call(port, "dispatch_key_event", {"windowHandle": window, "text": char, "eventType": "PressAndRelease"})


def find(port, element_id):
    window = main_window(port)
    found = first(call(port, "find_elements_by_id", {"windowHandle": window, "elementsId": element_id}))
    return found.get("elementHandles", []) if isinstance(found, dict) else found


def main(argv):
    port = int(os.environ.get("SLINT_MCP_PORT", DEFAULT_PORT))
    global WINDOW
    if len(argv) >= 2 and argv[0] == "--port":
        port, argv = int(argv[1]), argv[2:]
    if len(argv) >= 2 and argv[0] == "--window":
        WINDOW, argv = int(argv[1]), argv[2:]
    if not argv:
        print(__doc__)
        return 1
    command, rest = argv[0], argv[1:]
    if command == "tools":
        for tool in rpc(port, "tools/list").get("tools", []):
            print(f"{tool['name']}: {tool.get('description', '')}")
    elif command == "call":
        arguments = json.loads(rest[1]) if len(rest) > 1 else {}
        print(json.dumps(call(port, rest[0], arguments), indent=2))
    elif command == "tree":
        window = main_window(port)
        props = first(call(port, "get_window_properties", {"windowHandle": window}))
        tree = first(
            call(
                port,
                "get_element_tree",
                {"elementHandle": props["rootElementHandle"], "maxElements": int(rest[0]) if rest else 200},
            )
        )
        print(json.dumps(tree, indent=1))
    elif command == "find":
        print(json.dumps(find(port, rest[0]), indent=2))
    elif command == "click":
        handles = find(port, rest[0])
        if not handles:
            raise RuntimeError(f"no element {rest[0]}")
        print(json.dumps(call(port, "click_element", {"elementHandle": handles[0]})))
    elif command == "click-nth":
        handles = find(port, rest[0])
        index = int(rest[1])
        if index >= len(handles):
            raise RuntimeError(f"only {len(handles)} elements {rest[0]}")
        arguments = {"elementHandle": handles[index]}
        if len(rest) > 2 and rest[2] == "double":
            arguments["action"] = "DoubleClick"
        print(json.dumps(call(port, "click_element", arguments)))
    elif command == "click-type":
        window = main_window(port)
        props = first(call(port, "get_window_properties", {"windowHandle": window}))
        found = first(
            call(
                port,
                "query_element_descendants",
                {
                    "elementHandle": props["rootElementHandle"],
                    "queryStack": [{"matchDescendants": True}, {"matchElementTypeName": rest[0]}],
                    "findAll": True,
                },
            )
        )
        handles = found.get("elementHandles", []) if isinstance(found, dict) else found
        # The server may list a handle more than once.
        unique = []
        for handle in handles:
            if handle not in unique:
                unique.append(handle)
        index = int(rest[1])
        if index >= len(unique):
            raise RuntimeError(f"only {len(unique)} elements of type {rest[0]}")
        print(json.dumps(call(port, "click_element", {"elementHandle": unique[index]})))
    elif command == "move":
        window = main_window(port)
        position = {"x": float(rest[0]), "y": float(rest[1])}
        print(json.dumps(call(port, "move_pointer", {"windowHandle": window, "position": position})))
    elif command == "key":
        key(port, rest[0])
    elif command == "type":
        type_text(port, rest[0])
    elif command == "shot":
        window = main_window(port)
        for block in call(port, "take_screenshot", {"windowHandle": window, "imageMimeType": "image/png"}):
            if isinstance(block, dict) and block.get("type") == "image":
                with open(rest[0], "wb") as out:
                    out.write(base64.b64decode(block["data"]))
                print(rest[0])
                return 0
        raise RuntimeError("no image in the answer")
    else:
        print(__doc__)
        return 1
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except (OSError, RuntimeError, ValueError, KeyError, IndexError) as error:
        print(f"mcp.py: {error}", file=sys.stderr)
        sys.exit(1)
