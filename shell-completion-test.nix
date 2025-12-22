{
  runCommand,
  bash,
  bash-completion,
  zsh,
  fish,
  nushell,
  cntr,
}:

runCommand "shell-completion-test"
  {
    nativeBuildInputs = [
      bash
      bash-completion
      zsh
      fish
      nushell
    ];
  }
  ''
    set -euo pipefail

    echo "=== Testing bash completion ==="
    bash -c '
      source ${bash-completion}/share/bash-completion/bash_completion
      source ${cntr}/share/bash-completion/completions/cntr.bash

      # Test that completion function exists
      type _cntr

      # Test completion for subcommands
      COMP_WORDS=(cntr "")
      COMP_CWORD=1
      _cntr
      echo "Bash completions: ''${COMPREPLY[*]}"
      [[ " ''${COMPREPLY[*]} " == *" attach "* ]] || { echo "Missing attach"; exit 1; }
      [[ " ''${COMPREPLY[*]} " == *" exec "* ]] || { echo "Missing exec"; exit 1; }
      [[ " ''${COMPREPLY[*]} " == *" help "* ]] || { echo "Missing help"; exit 1; }
      [[ " ''${COMPREPLY[*]} " == *" version "* ]] || { echo "Missing version"; exit 1; }
    '
    echo "=== Testing zsh completion ==="
    zsh -f -c '
      autoload -U compinit && compinit -u
      source ${cntr}/share/zsh/site-functions/_cntr

      # Test that completion function exists
      whence -v _cntr | grep -q "function"
    '
    echo "=== Testing fish completion ==="
    fish --no-config -c '
      source ${cntr}/share/fish/vendor_completions.d/cntr.fish

      # Test that completions are registered
      complete -c cntr | grep -q attach
      complete -c cntr | grep -q exec
    '
    echo "=== Testing nushell completion ==="
    nu --no-config-file -c '
      source ${cntr}/share/nushell/vendor/autoload/cntr.nu

      # Test that the command is defined with completions
      help cntr | str contains "attach"
      help cntr | str contains "exec"
    '
    touch $out
  ''
