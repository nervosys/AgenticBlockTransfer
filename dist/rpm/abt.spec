Name:           abt
Version:        1.0.0
Release:        1%{?dist}
Summary:        Agentic-first CLI successor to dd with GUI/TUI
License:        MIT OR Apache-2.0
URL:            https://github.com/nervosys/AgenticBlockTransfer
Source0:        https://github.com/nervosys/AgenticBlockTransfer/archive/refs/tags/v%{version}.tar.gz

BuildRequires:  rust >= 1.75
BuildRequires:  cargo

%description
AgenticBlockTransfer (abt) is a cross-platform disk image writer with CLI,
TUI, and GUI modes. It provides safe, verified block-level writes with
AI-discoverable ontology, multi-format support (ISO, IMG, QCOW2, VHD,
VMDK, WIM), auto-decompression, device cloning, secure erase, multicast
imaging, multi-boot USB, localization, and accessibility.

%prep
%autosetup -n AgenticBlockTransfer-%{version}

%build
cargo build --release --all-features

%install
install -Dm755 target/release/abt %{buildroot}%{_bindir}/abt

# Shell completions
mkdir -p %{buildroot}%{_datadir}/bash-completion/completions
target/release/abt completions bash > %{buildroot}%{_datadir}/bash-completion/completions/abt

mkdir -p %{buildroot}%{_datadir}/zsh/site-functions
target/release/abt completions zsh > %{buildroot}%{_datadir}/zsh/site-functions/_abt

mkdir -p %{buildroot}%{_datadir}/fish/vendor_completions.d
target/release/abt completions fish > %{buildroot}%{_datadir}/fish/vendor_completions.d/abt.fish

# Man pages
mkdir -p %{buildroot}%{_mandir}/man1
target/release/abt man --output-dir %{buildroot}%{_mandir}/man1

%check
cargo test --all-features

%files
%license LICENSE-MIT LICENSE-APACHE
%doc README.md CHANGELOG.md
%{_bindir}/abt
%{_datadir}/bash-completion/completions/abt
%{_datadir}/zsh/site-functions/_abt
%{_datadir}/fish/vendor_completions.d/abt.fish
%{_mandir}/man1/abt*.1*

%changelog
* Fri Feb 28 2026 nervosys <https://github.com/nervosys> - 1.0.0-1
- Initial RPM release
