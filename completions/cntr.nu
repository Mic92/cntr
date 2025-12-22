# Nushell completions for cntr

def "nu-complete cntr container-types" [] {
    [
        { value: "process_id", description: "Direct process ID" }
        { value: "podman", description: "Podman containers" }
        { value: "docker", description: "Docker containers" }
        { value: "nspawn", description: "systemd-nspawn containers" }
        { value: "lxc", description: "LXC containers" }
        { value: "lxd", description: "LXD containers" }
        { value: "containerd", description: "containerd containers" }
        { value: "command", description: "Execute a command to get PID" }
        { value: "kubernetes", description: "Kubernetes containers" }
    ]
}

def "nu-complete cntr apparmor-modes" [] {
    [
        { value: "auto", description: "Automatically handle AppArmor profiles" }
        { value: "off", description: "Disable AppArmor profile transitions" }
    ]
}

def "nu-complete cntr containers" [] {
    mut containers = []

    # Docker containers
    if (which docker | is-not-empty) {
        try {
            $containers = ($containers | append (docker ps --format '{{.Names}}' | lines))
        }
    }

    # Podman containers
    if (which podman | is-not-empty) {
        try {
            $containers = ($containers | append (podman ps --format '{{.Names}}' | lines))
        }
    }

    # containerd tasks (via ctr)
    if (which ctr | is-not-empty) {
        try {
            # ctr task list output: TASK PID STATUS (skip header)
            $containers = ($containers | append (ctr task list | lines | skip 1 | each { |line| $line | split row -r '\s+' | first }))
        }
    }

    # containerd containers (via nerdctl)
    if (which nerdctl | is-not-empty) {
        try {
            $containers = ($containers | append (nerdctl ps --format '{{.Names}}' | lines))
        }
    }

    # systemd-nspawn machines
    if (which machinectl | is-not-empty) {
        try {
            $containers = ($containers | append (machinectl list --no-legend | lines | each { |line| $line | split row ' ' | first }))
        }
    }

    # LXC containers (lxc-ls) or LXD containers (lxc)
    if (which lxc-ls | is-not-empty) {
        try {
            $containers = ($containers | append (lxc-ls --running | lines))
        }
    } else if (which lxc | is-not-empty) {
        try {
            $containers = ($containers | append (lxc list --format csv -c n status=running | lines))
        }
    }

    # Kubernetes pods (only for single-node clusters like minikube, kind, k3s, docker-desktop)
    if (which kubectl | is-not-empty) {
        try {
            let node_count = (kubectl get nodes --no-headers | lines | length)
            if $node_count == 1 {
                $containers = ($containers | append (
                    kubectl get pods --all-namespaces --no-headers -o custom-columns=':metadata.namespace,:metadata.name'
                    | lines
                    | each { |line|
                        let parts = ($line | split row -r '\s+')
                        if ($parts | first) == "default" {
                            $parts | last
                        } else {
                            $"($parts | first)/($parts | last)"
                        }
                    }
                ))
            }
        }
    }

    $containers | uniq
}

# A container debugging tool based on Linux mount API
export extern "cntr" [
    --help(-h)     # Print help
    --version(-V)  # Print version
]

# Enter container with mount overlay
export extern "cntr attach" [
    container: string@"nu-complete cntr containers"  # Container ID, name, or process ID
    --type(-t): string@"nu-complete cntr container-types"  # Container types to try
    --apparmor: string@"nu-complete cntr apparmor-modes"   # AppArmor profile mode
    --effective-user: string  # Effective username for new files
    --help(-h)     # Print help
    --version(-V)  # Print version
    ...command: string  # Command to execute
]

# Execute command in container
export extern "cntr exec" [
    container: string@"nu-complete cntr containers"  # Container ID, name, or process ID
    --type(-t): string@"nu-complete cntr container-types"  # Container types to try
    --apparmor: string@"nu-complete cntr apparmor-modes"   # AppArmor profile mode
    --help(-h)     # Print help
    --version(-V)  # Print version
    ...command: string  # Command to execute
]

# Print help
export extern "cntr help" []

# Print version
export extern "cntr version" []
