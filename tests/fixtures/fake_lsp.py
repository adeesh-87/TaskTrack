#!/usr/bin/env python3
"""A tiny language server for pahiri's tests: `fake_lsp.py LOG`.

Diagnoses every "bad" as an error; the definition of anything is the line
starting with "int helper"; references are the lines mentioning "helper";
it offers two completions, one symbol per workspace query (QUERY_sym) and
the "int NAME(" lines of a file as its symbols. Every method received is
appended to LOG (answers to its own requests as "reply <id> <result>").
"""
import json
import sys

log = open(sys.argv[1], "a")
docs = {}


def read():
    length = None
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        line = line.decode().strip()
        if not line:
            break
        key, value = line.split(":", 1)
        if key.lower() == "content-length":
            length = int(value)
    return json.loads(sys.stdin.buffer.read(length))


def send(msg):
    msg["jsonrpc"] = "2.0"
    body = json.dumps(msg).encode()
    sys.stdout.buffer.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
    sys.stdout.buffer.flush()


def rng(line, start, end):
    return {"start": {"line": line, "character": start}, "end": {"line": line, "character": end}}


while True:
    m = read()
    if m is None:
        break
    method, mid = m.get("method"), m.get("id")
    if method is None:
        log.write("reply %s %s\n" % (mid, json.dumps(m.get("result"))))
    else:
        log.write(method + "\n")
    log.flush()
    params = m.get("params") or {}
    uri = params.get("textDocument", {}).get("uri")
    lines = docs.get(uri, "").split("\n")
    if method == "initialize":
        send({"id": mid, "result": {"capabilities": {
            "positionEncoding": "utf-32", "textDocumentSync": 1,
            "definitionProvider": True, "referencesProvider": True, "hoverProvider": True,
            "completionProvider": {}, "workspaceSymbolProvider": True,
            "documentSymbolProvider": True}}})
    elif method == "initialized":
        send({"id": "cfg1", "method": "workspace/configuration", "params": {"items": [{}, {}]}})
        send({"method": "$/progress", "params": {"token": "i", "value": {"kind": "begin", "title": "Indexing", "percentage": 40}}})
    elif method in ("textDocument/didOpen", "textDocument/didChange"):
        text = params["textDocument"]["text"] if method.endswith("didOpen") else params["contentChanges"][-1]["text"]
        docs[uri] = text
        diags = []
        for i, line in enumerate(text.split("\n")):
            c = line.find("bad")
            if c >= 0:
                diags.append({"range": rng(i, c, c + 3), "severity": 1, "message": "bad is bad", "source": "fake"})
        send({"method": "textDocument/publishDiagnostics", "params": {"uri": uri, "diagnostics": diags}})
    elif method == "textDocument/definition":
        line = next(i for i, l in enumerate(lines) if l.startswith("int helper"))
        send({"id": mid, "result": [{"uri": uri, "range": rng(line, 4, 10)}]})
    elif method == "textDocument/references":
        send({"id": mid, "result": [{"uri": uri, "range": rng(i, l.find("helper"), l.find("helper") + 6)}
                                     for i, l in enumerate(lines) if "helper" in l]})
    elif method == "textDocument/hover":
        send({"id": mid, "result": {"contents": {"kind": "markdown", "value": "```c\nint helper(void)\n```\nReturns one."}}})
    elif method == "textDocument/completion":
        send({"id": mid, "result": {"isIncomplete": False, "items": [
            {"label": "helper_one", "detail": "int", "sortText": "1"},
            {"label": "helper_two", "insertText": "helper_two", "detail": "void", "sortText": "2"}]}})
    elif method == "workspace/symbol":
        any_uri = next(iter(docs))
        send({"id": mid, "result": [{"name": params["query"] + "_sym", "kind": 12,
                                     "location": {"uri": any_uri, "range": rng(0, 4, 10)}}]})
    elif method == "textDocument/documentSymbol":
        syms = []
        for i, line in enumerate(lines):
            if line.startswith("int ") and "(" in line:
                name = line[4:line.index("(")]
                syms.append({"name": name, "kind": 12, "range": rng(i, 0, len(line)), "selectionRange": rng(i, 4, 4 + len(name))})
        send({"id": mid, "result": syms})
    elif method == "shutdown":
        send({"id": mid, "result": None})
    elif method == "exit":
        break
