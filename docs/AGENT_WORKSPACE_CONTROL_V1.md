# Phoenix Agent Workspace Control V1

Status: native vertical slice, OpenRouter execution deferred.

Phoenix exposes a typed, bash-like `phx` command language through a local-only
Windows named pipe. The running application remains the sole workspace writer;
`phoenixctl` is a client and never opens the durable stores for mutation.

## Commands

```text
phx app status
phx workspace ls
phx note stat [note://ID]
phx note cat [note://ID] [--from BYTE] [--to BYTE]
phx block ls [note://ID] [--from INDEX] [--limit 1..256]
phx block insert [note://ID] [--after UUID] --text MARKDOWN \
  --expected-document-rev REV --idempotency-key KEY
phx events after SEQUENCE
```

`block ls` defaults to 128 blocks and returns `next_from` when another page is
available. Note ranges are UTF-8 byte ranges and reject split code points. An
omitted `--to` returns at most 64 KiB; explicit reads are capped at 512 KiB and
return `next_from` when more content remains.

## Mutation contract

Block insertion currently targets the active note only. It enters through the
resident Velotype agent-operation boundary, commits through the kernel's
document lease, and restores the editor if the durable commit fails.

Every write requires the document revision observed by a prior read. Stale
revisions fail with `conflict`. The idempotency key is durably bound to the
canonical command, including a BLAKE3 digest of the Markdown payload. Repeating
the same command returns the original receipt with `replayed: true`; reusing the
key for different content fails closed.

## Transport and event semantics

- The endpoint is a randomized local named pipe with remote clients rejected.
- A descriptor beside the workspace manifest identifies the live PID and pipe.
- Clients delete a descriptor only after proving its PID is no longer alive.
- Requests and inline responses are capped at 1 MiB.
- The UI host queue is bounded at 32 and uses non-blocking admission.
- Kernel events are observed by sequence cursor. Reads never drain another
  consumer's events.
- The kernel event ring remains capped at 256. A slow observer receives an
  explicit `gap` plus the oldest available sequence after eviction; it cannot
  freeze graph commands.

## Deferred from this cut

- OpenRouter API-key and model/tool-call qualification.
- Non-active-note block mutation.
- Note/folder create, rename, move, and delete commands.
- Graph search and bounded recursive memory-working-set commands.
- Agent policy/permission profiles beyond the local user boundary.
