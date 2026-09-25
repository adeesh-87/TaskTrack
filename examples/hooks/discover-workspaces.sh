#!/bin/sh
# startup hook: fill in code workspaces and vendor builds from what is on
# disk. Re-run it any time with Esc ! → startup; pahiri reloads the config
# when it changes (no restart).
#   [hooks]
#   startup = "~/src/pahiri/examples/hooks/discover-workspaces.sh"
#
# What it finds (change the roots with environment variables, e.g. in the
# hook command: startup = "CODE_ROOTS=~/src:~/work ~/…/discover-workspaces.sh"):
#   CODE_ROOTS   git checkouts: folders with a .git, up to CODE_DEPTH (2) levels
#                down. Default: ~/src:~/work:~/code
#   BUILD_ROOTS  bitbake/yocto build folders: folders with conf/local.conf, up to
#                BUILD_DEPTH (3) levels down. Default: ~/builds:~/yocto
#   PRUNE=0      keep entries whose folder is gone (default: remove them)
#
# Names are the folder name (made safe: letters, digits, - _ .); when two
# folders share a name the parent folder is prefixed (vendor-linux). Existing
# entries keep their name and main branch; a moved folder with the same name
# gets its new path. Everything goes through `pahiri config …`, which only
# writes when something changed and refuses to write an invalid config.

set -f  # no globbing: paths are data
bin=${PAHIRI_BIN:-pahiri}
code_roots=${CODE_ROOTS:-$HOME/src:$HOME/work:$HOME/code}
build_roots=${BUILD_ROOTS:-$HOME/builds:$HOME/yocto}
code_depth=${CODE_DEPTH:-2}
build_depth=${BUILD_DEPTH:-3}

used='|'
repos=''
added=0
found=0
failed=0
nl='
'

# safe NAME: keep letters, digits, - _ . ; no leading - or .
safe() {
  printf '%s' "$1" | tr -c 'A-Za-z0-9_.-' '-' | sed 's/^[-.]*//'
}

# pick_name DIR: sets $name to the folder name, or parent-name when that is
# already used in this run.
pick_name() {
  name=$(safe "$(basename "$1")")
  case "$used" in
    *"|$name|"*) name=$(safe "$(basename "$(dirname "$1")")-$(basename "$1")") ;;
  esac
  used="$used$name|"
}

# nested DIR: whether DIR is inside a checkout found earlier (a submodule or
# a vendored repo), which is not a workspace of its own.
nested() {
  old_ifs=$IFS
  IFS=$nl
  for r in $repos; do
    case "$1" in "$r"/*) IFS=$old_ifs; return 0 ;; esac
  done
  IFS=$old_ifs
  return 1
}

# add KIND DIR: register one folder, counting what happened.
add() {
  pick_name "$2"
  [ -n "$name" ] || return
  found=$((found + 1))
  if out=$("$bin" config "add-$1" "$name" "$2" 2>&1); then
    case "$out" in *": added "*) added=$((added + 1)); echo "$out" ;; esac
  else
    failed=$((failed + 1))
    echo "skipped $2: $out" >&2
  fi
}

# roots LIST: the existing folders of a colon separated list, one per line.
roots() {
  printf '%s\n' "$1" | tr ':' '\n' | while IFS= read -r r; do
    case "$r" in "~"*) r="$HOME${r#\~}" ;; esac
    [ -d "$r" ] && printf '%s\n' "$r"
  done
}

tmp=$(mktemp) || exit 1
trap 'rm -f "$tmp"' EXIT

# Code workspaces (the .git may be a folder, or a file for worktrees).
roots "$code_roots" | while IFS= read -r root; do
  find "$root" -mindepth 2 -maxdepth $((code_depth + 1)) -name .git 2>/dev/null
done | sort | while IFS= read -r git; do dirname "$git"; done > "$tmp"
while IFS= read -r dir; do
  nested "$dir" && continue
  repos="$repos$dir$nl"
  add workspace "$dir"
done < "$tmp"
code_found=$found
code_added=$added

# Vendor builds.
found=0
added=0
roots "$build_roots" | while IFS= read -r root; do
  find "$root" -mindepth 2 -maxdepth $((build_depth + 2)) -path '*/conf/local.conf' 2>/dev/null
done | sort | while IFS= read -r conf; do dirname "$(dirname "$conf")"; done > "$tmp"
while IFS= read -r dir; do
  add build "$dir"
done < "$tmp"

pruned=""
if [ "${PRUNE:-1}" != 0 ]; then
  pruned=$("$bin" config prune 2>&1)
  [ "$pruned" = "nothing to prune" ] && pruned=""
fi

# The last line becomes pahiri's status line.
summary="found $code_found workspaces ($code_added new), $found builds ($added new)"
[ "$failed" -gt 0 ] && summary="$summary, $failed skipped"
[ -n "$pruned" ] && summary="$summary; $pruned"
echo "$summary"
