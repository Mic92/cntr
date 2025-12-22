Name:           cntr
Version:        2.0.0
Release:        1%{?dist}
Summary:        A container debugging tool based on Linux mount API

License:        MIT
URL:            https://github.com/Mic92/cntr
Source0:        %{url}/archive/refs/tags/%{version}.tar.gz#/%{name}-%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  rust >= 1.85
BuildRequires:  scdoc

ExclusiveArch:  x86_64 aarch64

%description
cntr is a replacement for `docker exec` that brings all your developer tools
with you by mounting the file system from one container or the host into
the target container using the Linux mount API.

This allows shipping minimal runtime images in production while still having
access to debugging tools when needed.

%prep
%autosetup -n %{name}-%{version}

%build
cargo build --release --locked
scdoc < doc/cntr.1.scd > cntr.1

%install
install -Dm755 target/release/cntr %{buildroot}%{_bindir}/cntr

# Shell completions
install -Dm644 completions/cntr.bash %{buildroot}%{_datadir}/bash-completion/completions/cntr
install -Dm644 completions/cntr.zsh %{buildroot}%{_datadir}/zsh/site-functions/_cntr
install -Dm644 completions/cntr.fish %{buildroot}%{_datadir}/fish/vendor_completions.d/cntr.fish
install -Dm644 completions/cntr.nu %{buildroot}%{_datadir}/nushell/completions/cntr.nu

# Manpage
install -Dm644 cntr.1 %{buildroot}%{_mandir}/man1/cntr.1

%check
cargo test --release --locked

%files
%license LICENSE.md
%doc README.md
%{_bindir}/cntr
%{_datadir}/bash-completion/completions/cntr
%{_datadir}/zsh/site-functions/_cntr
%{_datadir}/fish/vendor_completions.d/cntr.fish
%{_datadir}/nushell/completions/cntr.nu
%{_mandir}/man1/cntr.1*

%changelog
* Sun Dec 22 2024 Jörg Thalheim <joerg@thalheim.io> - 2.0.0-1
- Rewrite to use Linux mount API instead of FUSE
- Requires Linux kernel 5.2 or later
