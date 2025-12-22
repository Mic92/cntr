#compdef cntr

_cntr_container_types() {
    local types=(
        'process_id:Direct process ID'
        'podman:Podman containers'
        'docker:Docker containers'
        'nspawn:systemd-nspawn containers'
        'lxc:LXC containers'
        'lxd:LXD containers'
        'containerd:containerd containers'
        'command:Execute a command to get PID'
        'kubernetes:Kubernetes containers'
    )
    _describe -t types 'container type' types
}

_cntr_apparmor_modes() {
    local modes=(
        'auto:Automatically handle AppArmor profiles'
        'off:Disable AppArmor profile transitions'
    )
    _describe -t modes 'apparmor mode' modes
}

_cntr_containers() {
    local containers=()

    # Docker containers
    if (( $+commands[docker] )); then
        containers+=("${(@f)$(docker ps --format '{{.Names}}' 2>/dev/null)}")
    fi

    # Podman containers
    if (( $+commands[podman] )); then
        containers+=("${(@f)$(podman ps --format '{{.Names}}' 2>/dev/null)}")
    fi

    # containerd tasks (via ctr)
    if (( $+commands[ctr] )); then
        # ctr task list output: TASK PID STATUS
        containers+=("${(@f)$(ctr task list 2>/dev/null | tail -n +2 | awk '{print $1}')}")
    fi

    # containerd containers (via nerdctl)
    if (( $+commands[nerdctl] )); then
        containers+=("${(@f)$(nerdctl ps --format '{{.Names}}' 2>/dev/null)}")
    fi

    # systemd-nspawn machines
    if (( $+commands[machinectl] )); then
        containers+=("${(@f)$(machinectl list --no-legend 2>/dev/null | awk '{print $1}')}")
    fi

    # LXC containers (lxc-ls) or LXD containers (lxc)
    if (( $+commands[lxc-ls] )); then
        containers+=("${(@f)$(lxc-ls --running 2>/dev/null)}")
    elif (( $+commands[lxc] )); then
        containers+=("${(@f)$(lxc list --format csv -c n status=running 2>/dev/null)}")
    fi

    # Kubernetes pods (only for single-node clusters like minikube, kind, k3s, docker-desktop)
    if (( $+commands[kubectl] )); then
        local node_count
        node_count=$(kubectl get nodes --no-headers 2>/dev/null | wc -l)
        if [[ "$node_count" -eq 1 ]]; then
            containers+=("${(@f)$(kubectl get pods --all-namespaces --no-headers -o custom-columns=':metadata.namespace,:metadata.name' 2>/dev/null | \
                awk '{if ($1 == "default") print $2; else print $1"/"$2}')}")
        fi
    fi

    _describe -t containers 'container' containers
}

_cntr_attach() {
    _arguments -s \
        '(-t --type)'{-t,--type}'[Container types to try]:types:_cntr_container_types' \
        '--apparmor[AppArmor profile mode]:mode:_cntr_apparmor_modes' \
        '--effective-user[Effective username for new files]:user:_users' \
        '(-h --help)'{-h,--help}'[Print help]' \
        '(-V --version)'{-V,--version}'[Print version]' \
        ':container:_cntr_containers' \
        '*::command:_command_names -e'
}

_cntr_exec() {
    _arguments -s \
        '(-t --type)'{-t,--type}'[Container types to try]:types:_cntr_container_types' \
        '--apparmor[AppArmor profile mode]:mode:_cntr_apparmor_modes' \
        '(-h --help)'{-h,--help}'[Print help]' \
        '(-V --version)'{-V,--version}'[Print version]' \
        ':container:_cntr_containers' \
        '*::command:_command_names -e'
}

_cntr() {
    local -a commands=(
        'attach:Enter container with mount overlay'
        'exec:Execute command in container'
        'help:Print help'
        'version:Print version'
    )

    _arguments -s \
        '(-h --help)'{-h,--help}'[Print help]' \
        '(-V --version)'{-V,--version}'[Print version]' \
        ':command:->command' \
        '*::args:->args'

    case "$state" in
        command)
            _describe -t commands 'cntr command' commands
            ;;
        args)
            case "${words[1]}" in
                attach)
                    _cntr_attach
                    ;;
                exec)
                    _cntr_exec
                    ;;
            esac
            ;;
    esac
}

_cntr "$@"
