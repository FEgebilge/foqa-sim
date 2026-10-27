# Devlog

One file per release: `devlog/<major.minor.patch>.md`, for example `0.1.2.md`. No code to touch.

```
---
version: 0.1.2
date: 2026-10-27
title: A short headline
---
```

Below the block, plain Markdown: `##` headings (optional), `-` bullet lists, **bold**, `code`, [links](https://example.com).
Write for pilots, not developers. The app bundles every file at build time, shows the new ones in a dialog after an
update and lists them all on the Devlog page. `npm run release -- <version>` (in `app/`) refuses to run unless
`devlog/<version>.md` exists and its `version:` matches; the same text is meant to become the GitHub release notes.
