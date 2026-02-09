# Issue: stale listener state window in FileManager

## Problem
`FileManager` updated listener refs (`selectedNodeRef`, `isRootSelectedRef`, `reloadFilesRef`) inside a `useEffect`.

Because `useEffect` runs after paint, there is a small interval after a render where long-lived listeners (`tree_updated`, drag-and-drop `drop`) can still read values from the previous render. In practice, this can skip refreshes or apply uploads against an outdated folder selection when external events fire immediately after state changes.

## Fix
Assign latest values to refs directly during render:
- `selectedNodeRef.current = selectedNode`
- `isRootSelectedRef.current = isRootSelected`
- `reloadFilesRef.current = reloadFiles`

This keeps listener reads synchronized with the current render, while preserving stable subscriptions.

## Verification
- `npm test`
- `npm --workspace ui run build`
