#!/bin/bash -ex

PREFIX="${PREFIX:-/usr/local}"

# install plugins
install -d "${PREFIX}/libexec/ciel-plugin"
install -Dvm755 plugins/* "${PREFIX}/libexec/ciel-plugin"

# install completions
install -dv "${PREFIX}/share/zsh/functions/Completion/Linux/"
install -Dvm644 cli/completions/_ciel "${PREFIX}/share/zsh/functions/Completion/Linux/"
install -dv "${PREFIX}/share/fish/vendor_completions.d/"
install -Dvm644 cli/completions/ciel.fish "${PREFIX}/share/fish/vendor_completions.d/"
install -dv "${PREFIX}/share/bash-completion/completions/"
install -Dvm644 cli/completions/ciel.bash "${PREFIX}/share/bash-completion/completions/"
