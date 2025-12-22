# bash completion for cntr

_cntr_container_types() {
    echo "process_id podman docker nspawn lxc lxd containerd command kubernetes"
}

_cntr_apparmor_modes() {
    echo "auto off"
}

_cntr_containers() {
    local containers=""

    # Docker containers
    if command -v docker &>/dev/null; then
        containers+=" $(docker ps --format '{{.Names}}' 2>/dev/null)"
    fi

    # Podman containers
    if command -v podman &>/dev/null; then
        containers+=" $(podman ps --format '{{.Names}}' 2>/dev/null)"
    fi

    # containerd tasks (via ctr)
    if command -v ctr &>/dev/null; then
        # ctr task list output: TASK PID STATUS
        containers+=" $(ctr task list 2>/dev/null | tail -n +2 | awk '{print $1}')"
    fi

    # containerd containers (via nerdctl)
    if command -v nerdctl &>/dev/null; then
        containers+=" $(nerdctl ps --format '{{.Names}}' 2>/dev/null)"
    fi

    # systemd-nspawn machines
    if command -v machinectl &>/dev/null; then
        containers+=" $(machinectl list --no-legend 2>/dev/null | awk '{print $1}')"
    fi

    # LXC containers (lxc-ls) or LXD containers (lxc)
    if command -v lxc-ls &>/dev/null; then
        containers+=" $(lxc-ls --running 2>/dev/null)"
    elif command -v lxc &>/dev/null; then
        containers+=" $(lxc list --format csv -c n status=running 2>/dev/null)"
    fi

    # Kubernetes pods (only for single-node clusters like minikube, kind, k3s, docker-desktop)
    if command -v kubectl &>/dev/null; then
        local node_count
        node_count=$(kubectl get nodes --no-headers 2>/dev/null | wc -l)
        if [[ "$node_count" -eq 1 ]]; then
            containers+=" $(kubectl get pods --all-namespaces --no-headers -o custom-columns=':metadata.namespace,:metadata.name' 2>/dev/null | \
                awk '{if ($1 == "default") print $2; else print $1"/"$2}')"
        fi
    fi

    echo "$containers"
}

_cntr() {
    local cur prev words cword
    _init_completion || return

    local subcommands="attach exec help version"
    local common_opts="-t --type --apparmor -h --help -V --version"
    local attach_opts="--effective-user"

    # Find the subcommand
    local subcommand=""
    local i
    for ((i = 1; i < cword; i++)); do
        case "${words[i]}" in
            attach|exec|help|version)
                subcommand="${words[i]}"
                break
                ;;
        esac
    done

    case "$subcommand" in
        attach)
            case "$prev" in
                -t|--type)
                    COMPREPLY=($(compgen -W "$(_cntr_container_types)" -- "$cur"))
                    return
                    ;;
                --apparmor)
                    COMPREPLY=($(compgen -W "$(_cntr_apparmor_modes)" -- "$cur"))
                    return
                    ;;
                --effective-user)
                    COMPREPLY=($(compgen -u -- "$cur"))
                    return
                    ;;
            esac

            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "$common_opts $attach_opts" -- "$cur"))
            else
                COMPREPLY=($(compgen -W "$(_cntr_containers)" -- "$cur"))
            fi
            ;;
        exec)
            case "$prev" in
                -t|--type)
                    COMPREPLY=($(compgen -W "$(_cntr_container_types)" -- "$cur"))
                    return
                    ;;
                --apparmor)
                    COMPREPLY=($(compgen -W "$(_cntr_apparmor_modes)" -- "$cur"))
                    return
                    ;;
            esac

            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "$common_opts" -- "$cur"))
            else
                COMPREPLY=($(compgen -W "$(_cntr_containers)" -- "$cur"))
            fi
            ;;
        help|version)
            # No further completions
            ;;
        *)
            # Complete subcommand
            COMPREPLY=($(compgen -W "$subcommands" -- "$cur"))
            ;;
    esac
}

complete -F _cntr cntr
