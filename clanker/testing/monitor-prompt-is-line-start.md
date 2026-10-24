# The monitor prompt is `CRLF > SPACE`, not any `> `

## Symptom

A test client that reads the monitor until it sees `"> "` anywhere in the
stream returns an empty reply for the command *after* `machine-start`. A
`machine-status json` immediately after a `machine-start` acknowledgement is
the usual victim.

## Cause

`machine-start` acknowledges with an arrow:

```
machine-start: halted -> starting -> running
```

`" -> "` contains the bytes `"> "`. A naive prompt search matches the arrow,
returns before the real prompt arrives, and leaves the prompt in the socket.
The next command therefore reads the stale prompt as its whole response.

The same trap exists for any command output ending in an arrow, and for
`iris-ci status` output if a future field ever contains `"> "`.

## Rule

Detect the prompt as a line of its own — the bytes `\r\n> ` at the end of the
accumulated buffer — or use the existing `tools/mon.py`, whose
`(^|\n)> $` regex requires the prompt to start a line. Never search for a bare
`"> "` in a monitor reply.
