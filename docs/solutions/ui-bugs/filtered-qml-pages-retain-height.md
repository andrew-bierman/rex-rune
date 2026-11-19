---
title: Filtered QML settings pages retain their old height
date: 2026-11-15
category: ui-bugs
module: Qt Settings search
problem_type: ui_bug
component: frontend
symptoms:
  - Matching settings controls remain outside the clipped search viewport.
root_cause: wrong_api
resolution_type: code_fix
severity: medium
framework_version: Qt 6.12
tags: [qml, settings, search, layout, clipping]
---

# Filtered QML settings pages retain their old height

## Problem

Searching Settings for the Activity icon count found the right control but left it outside the visible scroll area. A successful text match did not establish that the control could receive a real click.

## Symptoms

The unchanged private Activity fixture timed out while trying to change the icon count. During diagnosis, the control's scene position was below the ScrollView's clipped bottom. The dialog remained responsive; the failure was layout geometry.

## What Didn't Work

Hiding unrelated terminal previews and explanatory labels reduced the content, but did not solve the complete fixture. Empty subpages still occupied their previous height, pushing later matching rows farther down. Increasing click timeouts would not correct that geometry. A partial search-only success was therefore insufficient evidence.

## Solution

Keep relevant previews visible and give each searchable subpage an explicit preferred height derived from its content:

```qml
TerminalSettings {
    query: search.text
    Layout.fillWidth: true
    Layout.preferredHeight: implicitHeight
}
```

All searchable subpages use this binding in [Settings.qml](../../../qt/Settings.qml). [TerminalSettings.qml](../../../qt/TerminalSettings.qml) filters supporting previews by the relevant query terms; unrelated help text is filtered in InterfaceSettings as well.

## Why This Works

Qt chooses a preferred size before considering implicit size. Zero implicit height is invalid as an implicit size hint, and layout fallback can use an item's allocated geometry. Consequently, hiding every child does not reliably collapse an already laid-out page. Giving the layout an explicit preferred height avoids this fallback. See Qt's [preferred-size rules](https://doc.qt.io/qt-6/qtquicklayouts-overview.html#specifying-preferred-size).

This was verified through complete existing fixtures, without relaxing their clicks or assertions: the Activity/icon fixture passed all seven runs and the preferences/ownership fixture also passed. Machine-local evidence is recorded in [the status document](../../qt-quick-status.md); these results cover the exercised settings and geometry rather than every possible query or display scale.

## Prevention

When a filtered page becomes empty, inspect both implicit and allocated height. Test search by activating the resulting control inside its clipped viewport, then changing the query and reopening the dialog. Retain the existing `scripts/qt-activity-private.py` and `scripts/qt-preferences-private.py` runtime checks; source-text assertions would miss the layout fallback. The failed preview-only approach and the reason for explicit preferred height are the durable lesson beyond those behavioral checks.

## Related Issues

The owned fork's issues are disabled. This lesson accompanies the unmerged downstream [PR #1](https://github.com/andrew-bierman/rex-rune/pull/1); it does not claim an upstream fix or release.
