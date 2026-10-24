# Security

## Reporting a vulnerability

Please report vulnerabilities privately through
[GitHub's private vulnerability reporting](https://github.com/Vironnimo/vWatch/security/advisories/new),
not in a public issue. Include the vWatch version (`vwatch --version`), what an
attacker can do and the steps to reproduce it. You will get an answer within a week.

## Scope

vWatch is a local Windows app. It reads your repositories with `git` (and
optionally `gh` for CI status), stores everything in a SQLite database in its
installation folder and opens no network port. The `vwatch` CLI has no
authentication: any program running under your Windows account can read and
change vWatch's data, which is how your coding agent writes its reports. That is
by design.

Reports about vWatch writing to a watched repository, running a program from
repository or card content, exposing data to another account or machine, or about
the installer are in scope.

Only the latest release receives fixes.
