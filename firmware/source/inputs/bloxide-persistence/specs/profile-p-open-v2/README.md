# P-OPEN review entry

Read [policy.md](policy.md), [reference-evidence.md](reference-evidence.md) and
[integration.md](integration.md). The policy is concrete and ready for independent
review, not accepted or activated. It retains one page and all43 prior wire pairs.
The unresolved standards meaning is explicitly scoped as two application-policy
approval decisions, not claimed solved by synchronous reference errors.

From the repository root:

```
python3 -B tools/profile-p-design-v1/check_design.py
python3 -B tools/profile-p-open/check_policy.py
/path/to/pinned-venv/bin/python -B tools/profile-p-open/check_metadata.py /owned/new-output
```

Reference characterization is source-specific host analysis. Public upstream
commit identities, hashes and technical observations are retained in
[sources.json](../../evidence/profile-p-open-v2/sources.json) and
[reference-evidence.md](reference-evidence.md). Historical fixture binaries,
raw execution captures and internal workspaces are outside the customer package.

The loopback reference experiment is distinct from the delivered CAN application.
It demonstrates upstream inconsistencies and loaded-file restoration within its
stated source/configuration scope; it does not establish physical flash durability.
For current application programming, calibration, recovery and verification, use
the customer engineering manual and `tools/firmware` / `tools/can` commands.
