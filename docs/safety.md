# Cleanup safety

Cleanup only moves explicitly confirmed items to the OS trash. System commands
are displayed and never executed. The same audited pipeline serves GUI and CLI.

## Evidence and refusal rules

- Every component of a candidate path must be free of symlinks/reparse points.
  Parent traversal (`..`) is refused for filesystem evidence.
- System roots, the profile root, sensitive directories, their ancestors and
  the audit directory cannot be selected. Windows also protects configured
  system directories and all drive/UNC roots.
- Snapshots retain full timestamp precision, identity, size and link count.
- Directory snapshots include every descendant's relative path and metadata.
  Errors, special files, links, mount boundaries and more than 100,000 entries
  reject the candidate rather than producing a partial review. Hardlinks count
  once toward its estimated size.
- Linux mountinfo detects bind mounts in addition to device changes.
- Revalidation runs before intent and again after its durable audit flush.
  Changed items are refused. A failed audit result stops further operations.

## Limits

The OS trash API operates on paths. Checks cannot make the final lookup/rename
atomic with validation, nor prevent concurrent writers modifying an already
checked file. This is not a security boundary against a malicious process with
write access to the same directories. Stop active builds/downloads before
reviewing their folders. Moving data to trash normally does not free disk space
until the user empties the trash separately.

Metadata checks cannot detect deliberately restored metadata. Duplicate cleanup
therefore rehashes both the reviewed copy and keeper with BLAKE3 before intent,
checks opened-handle identity, and rechecks their metadata after intent. Selection
uses scan-time snapshots; it does not silently approve changes since scanning.
Keepers and their parents cannot be trashed in the same batch. Concurrent writes
after the final check remain subject to the OS race described above.

Cancel/Escape in the GUI and Ctrl+C during CLI apply stop subsequent operations.
An in-flight OS trash call finishes and is audited before cancellation takes
effect. Directory revalidation and duplicate hashing also check cancellation.

Windows runtime tests remain required; cross-compilation does not validate OS
trash behavior or reparse-point handling on a real Windows filesystem.
