#!/usr/bin/env bash

CURRENT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"

DEFAULT_THUMBS_KEY=space

THUMBS_KEY="$(tmux show-option -gqv @thumbs-key)"
THUMBS_KEY=${THUMBS_KEY:-$DEFAULT_THUMBS_KEY}

COMMAND_ALIASES="$(tmux show-option -gqv command-alias)"

case ",${COMMAND_ALIASES}," in
  *,thumbs-pick=*) ;;
  *) tmux set-option -ag command-alias "thumbs-pick=run-shell -b ${CURRENT_DIR}/tmux-thumbs.sh" ;;
esac

tmux bind-key -N "Pick visible text with tmux-thumbs" "${THUMBS_KEY}" thumbs-pick
