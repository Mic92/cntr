# Fish completion for cntr

# Disable file completion by default
complete -c cntr -f

# Helper function to list running containers
function __cntr_containers
    # Docker containers
    if command -q docker
        docker ps --format '{{.Names}}' 2>/dev/null
    end

    # Podman containers
    if command -q podman
        podman ps --format '{{.Names}}' 2>/dev/null
    end

    # containerd tasks (via ctr)
    if command -q ctr
        # ctr task list output: TASK PID STATUS
        ctr task list 2>/dev/null | tail -n +2 | awk '{print $1}'
    end

    # containerd containers (via nerdctl)
    if command -q nerdctl
        nerdctl ps --format '{{.Names}}' 2>/dev/null
    end

    # systemd-nspawn machines
    if command -q machinectl
        machinectl list --no-legend 2>/dev/null | awk '{print $1}'
    end

    # LXC containers (lxc-ls) or LXD containers (lxc)
    if command -q lxc-ls
        lxc-ls --running 2>/dev/null
    else if command -q lxc
        lxc list --format csv -c n status=running 2>/dev/null
    end

    # Kubernetes pods (only for single-node clusters like minikube, kind, k3s, docker-desktop)
    if command -q kubectl
        set -l node_count (kubectl get nodes --no-headers 2>/dev/null | wc -l | string trim)
        if test "$node_count" -eq 1
            kubectl get pods --all-namespaces --no-headers -o custom-columns=':metadata.namespace,:metadata.name' 2>/dev/null | \
                awk '{if ($1 == "default") print $2; else print $1"/"$2}'
        end
    end
end

# Check if we're in a subcommand
function __cntr_needs_command
    set -l cmd (commandline -opc)
    set -l count (count $cmd)
    test $count -eq 1
end

function __cntr_using_subcommand
    set -l cmd (commandline -opc)
    set -l count (count $cmd)
    if test $count -lt 2
        return 1
    end
    test "$cmd[2]" = "$argv[1]"
end

# Subcommands
complete -c cntr -n __cntr_needs_command -a attach -d 'Enter container with mount overlay'
complete -c cntr -n __cntr_needs_command -a exec -d 'Execute command in container'
complete -c cntr -n __cntr_needs_command -a help -d 'Print help'
complete -c cntr -n __cntr_needs_command -a version -d 'Print version'

# Global options
complete -c cntr -s h -l help -d 'Print help'
complete -c cntr -s V -l version -d 'Print version'

# Container types with descriptions
complete -c cntr -n '__cntr_using_subcommand attach; or __cntr_using_subcommand exec' -s t -l type -x -d 'Container types to try'
complete -c cntr -n '__cntr_using_subcommand attach; or __cntr_using_subcommand exec' -s t -l type -xa 'process_id' -d 'Direct process ID'
complete -c cntr -n '__cntr_using_subcommand attach; or __cntr_using_subcommand exec' -s t -l type -xa 'podman' -d 'Podman containers'
complete -c cntr -n '__cntr_using_subcommand attach; or __cntr_using_subcommand exec' -s t -l type -xa 'docker' -d 'Docker containers'
complete -c cntr -n '__cntr_using_subcommand attach; or __cntr_using_subcommand exec' -s t -l type -xa 'nspawn' -d 'systemd-nspawn containers'
complete -c cntr -n '__cntr_using_subcommand attach; or __cntr_using_subcommand exec' -s t -l type -xa 'lxc' -d 'LXC containers'
complete -c cntr -n '__cntr_using_subcommand attach; or __cntr_using_subcommand exec' -s t -l type -xa 'lxd' -d 'LXD containers'
complete -c cntr -n '__cntr_using_subcommand attach; or __cntr_using_subcommand exec' -s t -l type -xa 'containerd' -d 'containerd containers'
complete -c cntr -n '__cntr_using_subcommand attach; or __cntr_using_subcommand exec' -s t -l type -xa 'command' -d 'Execute a command to get PID'
complete -c cntr -n '__cntr_using_subcommand attach; or __cntr_using_subcommand exec' -s t -l type -xa 'kubernetes' -d 'Kubernetes containers'

# AppArmor modes with descriptions
complete -c cntr -n '__cntr_using_subcommand attach; or __cntr_using_subcommand exec' -l apparmor -x -d 'AppArmor profile mode'
complete -c cntr -n '__cntr_using_subcommand attach; or __cntr_using_subcommand exec' -l apparmor -xa 'auto' -d 'Automatically handle AppArmor profiles'
complete -c cntr -n '__cntr_using_subcommand attach; or __cntr_using_subcommand exec' -l apparmor -xa 'off' -d 'Disable AppArmor profile transitions'

# attach-specific options
complete -c cntr -n '__cntr_using_subcommand attach' -l effective-user -xa '(__fish_complete_users)' -d 'Effective username for new files'

# Subcommand options
complete -c cntr -n '__cntr_using_subcommand attach; or __cntr_using_subcommand exec' -s h -l help -d 'Print help'
complete -c cntr -n '__cntr_using_subcommand attach; or __cntr_using_subcommand exec' -s V -l version -d 'Print version'

# Container completion for attach and exec
complete -c cntr -n '__cntr_using_subcommand attach; or __cntr_using_subcommand exec' -xa '(__cntr_containers)' -d 'Container'
