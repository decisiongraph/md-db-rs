#!/usr/bin/env bash
set -euo pipefail

# Auto-merge PRs by rebasing on main and resolving trivial conflicts
# in additive-only files (mod.rs, main.rs, lib.rs, Cargo.lock).
#
# Usage: ./scripts/auto_merge_prs.sh <pr_number> [pr_number ...]

if [ $# -eq 0 ]; then
  echo "Usage: $0 <pr_number> [pr_number ...]"
  exit 1
fi

for pr_num in "$@"; do
  echo "=== Processing PR #${pr_num} ==="

  branch=$(gh pr view "$pr_num" --json headRefName -q .headRefName)
  echo "Branch: ${branch}"

  git fetch origin "$branch"
  git fetch origin main

  git checkout -B "$branch" "origin/$branch"

  if git merge origin/main --no-gpg-sign -m "merge main into ${branch}"; then
    echo "Clean merge"
  else
    echo "Conflicts detected, attempting auto-resolve..."

    resolved_all=true
    for f in $(git diff --name-only --diff-filter=U); do
      case "$f" in
        */mod.rs|*/main.rs|*/lib.rs)
          echo "  Auto-resolving (theirs): $f"
          git checkout --theirs "$f"
          git add "$f"
          ;;
        Cargo.lock)
          echo "  Regenerating Cargo.lock"
          git checkout --theirs "$f"
          cargo check 2>/dev/null || true
          git add Cargo.lock
          ;;
        *)
          echo "  CANNOT auto-resolve: $f"
          resolved_all=false
          ;;
      esac
    done

    if [ "$resolved_all" = false ]; then
      echo "ERROR: Unresolvable conflicts in PR #${pr_num}, skipping"
      git merge --abort
      continue
    fi

    git commit --no-gpg-sign -m "merge main into ${branch}"
  fi

  git push origin "$branch"
  echo "Pushed ${branch}, merging PR #${pr_num}..."
  gh pr merge "$pr_num" --merge --admin
  echo "=== PR #${pr_num} merged ==="
  echo
done

echo "Done!"
