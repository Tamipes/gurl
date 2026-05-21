gurl_wrapper() {
  if [ "$1" = "auth" ]; then
    gh auth status || gh auth login
    exit_code=$?
    if [ "$exit_code" -eq 127 ]; then
      echo Using "nix-shell -p gh --run 'gh <args>''"
      nix-shell -p gh --run 'gh auth status ' || nix-shell -p gh --run 'gh auth login'

      GITHUB_TOKEN="$(nix-shell -p gh --run 'gh auth token')"
    else
      GITHUB_TOKEN="$(gh auth token)"
    fi

    export GITHUB_TOKEN

    NIX_CONFIG="access-tokens = github.com=$GITHUB_TOKEN"
    export NIX_CONFIG
  else
    \gurl
  fi
}

GURL_EVAL_SET=true

alias gurl=gurl_wrapper
