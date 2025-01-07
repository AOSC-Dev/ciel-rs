# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_ciel_global_optspecs
	string join \n C= q/quiet h/help V/version
end

function __fish_ciel_needs_command
	# Figure out if the current invocation already has a command.
	set -l cmd (commandline -opc)
	set -e cmd[1]
	argparse -s (__fish_ciel_global_optspecs) -- $cmd 2>/dev/null
	or return
	if set -q argv[1]
		# Also print the command, so this can be used to figure out what it is.
		echo $argv[1]
		return 1
	end
	return 0
end

function __fish_ciel_using_subcommand
	set -l cmd (__fish_ciel_needs_command)
	test -z "$cmd"
	and return 1
	contains -- $cmd[1] $argv
end

complete -c ciel -n "__fish_ciel_needs_command" -s C -d 'Set the CIEL! working directory' -r
complete -c ciel -n "__fish_ciel_needs_command" -s q -l quiet -d 'shhhhhh!'
complete -c ciel -n "__fish_ciel_needs_command" -s h -l help -d 'Print help'
complete -c ciel -n "__fish_ciel_needs_command" -s V -l version -d 'Print version'
complete -c ciel -n "__fish_ciel_needs_command" -f -a "version" -d 'Display the version of CIEL!'
complete -c ciel -n "__fish_ciel_needs_command" -f -a "list" -d 'List all instances in the workspace'
complete -c ciel -n "__fish_ciel_needs_command" -f -a "new" -d 'Create a new CIEL! workspace'
complete -c ciel -n "__fish_ciel_needs_command" -f -a "farewell" -d 'Remove everything related to CIEL!'
complete -c ciel -n "__fish_ciel_needs_command" -f -a "load-os" -d 'Unpack OS tarball or fetch the latest BuildKit'
complete -c ciel -n "__fish_ciel_needs_command" -f -a "update-os" -d 'Update the OS in the container'
complete -c ciel -n "__fish_ciel_needs_command" -f -a "instconf" -d 'Configure instances'
complete -c ciel -n "__fish_ciel_needs_command" -f -a "config" -d 'Configure workspace'
complete -c ciel -n "__fish_ciel_needs_command" -f -a "load-tree" -d 'Clone abbs tree from git'
complete -c ciel -n "__fish_ciel_needs_command" -f -a "add" -d 'Add a new instance'
complete -c ciel -n "__fish_ciel_needs_command" -f -a "del" -d 'Remove one or all instance'
complete -c ciel -n "__fish_ciel_needs_command" -f -a "mount" -d 'Mount one or all instance'
complete -c ciel -n "__fish_ciel_needs_command" -f -a "boot" -d 'Start one or all instance'
complete -c ciel -n "__fish_ciel_needs_command" -f -a "stop" -d 'Shutdown one or all instance'
complete -c ciel -n "__fish_ciel_needs_command" -f -a "down" -d 'Shutdown and unmount one or all instance'
complete -c ciel -n "__fish_ciel_needs_command" -f -a "rollback" -d 'Rollback one or all instance'
complete -c ciel -n "__fish_ciel_needs_command" -f -a "commit" -d 'Commit changes onto the underlying base system'
complete -c ciel -n "__fish_ciel_needs_command" -f -a "shell" -d 'Start an interactive shell or run a shell command'
complete -c ciel -n "__fish_ciel_needs_command" -f -a "run" -d 'Run a command in the container'
complete -c ciel -n "__fish_ciel_needs_command" -f -a "build" -d 'Build the packages using the specified instance'
complete -c ciel -n "__fish_ciel_needs_command" -f -a "repo" -d 'Local repository maintenance'
complete -c ciel -n "__fish_ciel_needs_command" -f -a "clean" -d 'Clean all the output directories and source cache directories'
complete -c ciel -n "__fish_ciel_needs_command" -f -a "diagnose" -d 'Diagnose problems (hopefully)'
complete -c ciel -n "__fish_ciel_needs_command" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c ciel -n "__fish_ciel_using_subcommand version" -s h -l help -d 'Print help'
complete -c ciel -n "__fish_ciel_using_subcommand list" -s h -l help -d 'Print help'
complete -c ciel -n "__fish_ciel_using_subcommand new" -l rootfs -d 'Specify the tarball or squashfs to load after initialization' -r
complete -c ciel -n "__fish_ciel_using_subcommand new" -l sha256 -d 'Specify the SHA-256 checksum of OS tarball' -r
complete -c ciel -n "__fish_ciel_using_subcommand new" -s a -l arch -d 'Specify the architecture of the workspace' -r
complete -c ciel -n "__fish_ciel_using_subcommand new" -l tree -d 'URL to the abbs tree git repository' -r
complete -c ciel -n "__fish_ciel_using_subcommand new" -s m -l maintainer -d 'Maintainer information' -r
complete -c ciel -n "__fish_ciel_using_subcommand new" -l dnssec -d 'Enable DNSSEC' -r -f -a "{true\t'',false\t''}"
complete -c ciel -n "__fish_ciel_using_subcommand new" -l local-repo -d 'Enable local package repository' -r -f -a "{true\t'',false\t''}"
complete -c ciel -n "__fish_ciel_using_subcommand new" -l source-cache -d 'Enable local source caches' -r -f -a "{true\t'',false\t''}"
complete -c ciel -n "__fish_ciel_using_subcommand new" -l branch-exclusive-output -d 'Use different OUTPUT directory for branches' -r -f -a "{true\t'',false\t''}"
complete -c ciel -n "__fish_ciel_using_subcommand new" -l volatile-mount -d 'Enable volatile mount' -r -f -a "{true\t'',false\t''}"
complete -c ciel -n "__fish_ciel_using_subcommand new" -l use-apt -d 'Force to use APT' -r -f -a "{true\t'',false\t''}"
complete -c ciel -n "__fish_ciel_using_subcommand new" -l add-repo -d 'Add an extra APT repository' -r
complete -c ciel -n "__fish_ciel_using_subcommand new" -l remove-repo -d 'Remove an extra APT repository' -r
complete -c ciel -n "__fish_ciel_using_subcommand new" -l add-nspawn-opt -d 'Add an extra nspawn option' -r
complete -c ciel -n "__fish_ciel_using_subcommand new" -l remove-nspawn-opt -d 'Remove an extra nspawn option' -r
complete -c ciel -n "__fish_ciel_using_subcommand new" -l no-load-os -d 'Don\'t load OS automatically after initialization'
complete -c ciel -n "__fish_ciel_using_subcommand new" -l no-load-tree -d 'Don\'t load abbs tree automatically after initialization'
complete -c ciel -n "__fish_ciel_using_subcommand new" -l unset-repo -d 'Remove all extra APT repository'
complete -c ciel -n "__fish_ciel_using_subcommand new" -l unset-nspawn-opt -d 'Remove all extra nspawn option'
complete -c ciel -n "__fish_ciel_using_subcommand new" -s h -l help -d 'Print help'
complete -c ciel -n "__fish_ciel_using_subcommand farewell" -s f -d 'Force perform deletion without user confirmation'
complete -c ciel -n "__fish_ciel_using_subcommand farewell" -s h -l help -d 'Print help'
complete -c ciel -n "__fish_ciel_using_subcommand load-os" -l sha256 -d 'Specify the SHA-256 checksum of OS tarball' -r
complete -c ciel -n "__fish_ciel_using_subcommand load-os" -s a -l arch -d 'Specify the target architecture for fetching OS tarball' -r
complete -c ciel -n "__fish_ciel_using_subcommand load-os" -s f -l force -d 'Force override the loaded system'
complete -c ciel -n "__fish_ciel_using_subcommand load-os" -s h -l help -d 'Print help'
complete -c ciel -n "__fish_ciel_using_subcommand update-os" -l local-repo -d 'Enable local package repository' -r -f -a "{true\t'',false\t''}"
complete -c ciel -n "__fish_ciel_using_subcommand update-os" -l tmpfs -d 'Enable tmpfs' -r -f -a "{true\t'',false\t''}"
complete -c ciel -n "__fish_ciel_using_subcommand update-os" -l tmpfs-size -d 'Size of tmpfs to use, in MiB' -r
complete -c ciel -n "__fish_ciel_using_subcommand update-os" -l ro-tree -d 'Mount TREE as read-only' -r -f -a "{true\t'',false\t''}"
complete -c ciel -n "__fish_ciel_using_subcommand update-os" -l output -d 'Path to output directory' -r -F
complete -c ciel -n "__fish_ciel_using_subcommand update-os" -l add-repo -d 'Add an extra APT repository' -r
complete -c ciel -n "__fish_ciel_using_subcommand update-os" -l remove-repo -d 'Remove an extra APT repository' -r
complete -c ciel -n "__fish_ciel_using_subcommand update-os" -l add-nspawn-opt -d 'Add an extra nspawn option' -r
complete -c ciel -n "__fish_ciel_using_subcommand update-os" -l remove-nspawn-opt -d 'Remove an extra nspawn option' -r
complete -c ciel -n "__fish_ciel_using_subcommand update-os" -l force-use-apt -d 'Use apt to update-os'
complete -c ciel -n "__fish_ciel_using_subcommand update-os" -l unset-tmpfs-size -d 'Reset tmpfs size to default'
complete -c ciel -n "__fish_ciel_using_subcommand update-os" -l unset-output -d 'Use default output directory'
complete -c ciel -n "__fish_ciel_using_subcommand update-os" -l unset-repo -d 'Remove all extra APT repository'
complete -c ciel -n "__fish_ciel_using_subcommand update-os" -l unset-nspawn-opt -d 'Remove all extra nspawn option'
complete -c ciel -n "__fish_ciel_using_subcommand update-os" -s h -l help -d 'Print help'
complete -c ciel -n "__fish_ciel_using_subcommand instconf" -s i -d 'Instance to be configured' -r
complete -c ciel -n "__fish_ciel_using_subcommand instconf" -l local-repo -d 'Enable local package repository' -r -f -a "{true\t'',false\t''}"
complete -c ciel -n "__fish_ciel_using_subcommand instconf" -l tmpfs -d 'Enable tmpfs' -r -f -a "{true\t'',false\t''}"
complete -c ciel -n "__fish_ciel_using_subcommand instconf" -l tmpfs-size -d 'Size of tmpfs to use, in MiB' -r
complete -c ciel -n "__fish_ciel_using_subcommand instconf" -l ro-tree -d 'Mount TREE as read-only' -r -f -a "{true\t'',false\t''}"
complete -c ciel -n "__fish_ciel_using_subcommand instconf" -l output -d 'Path to output directory' -r -F
complete -c ciel -n "__fish_ciel_using_subcommand instconf" -l add-repo -d 'Add an extra APT repository' -r
complete -c ciel -n "__fish_ciel_using_subcommand instconf" -l remove-repo -d 'Remove an extra APT repository' -r
complete -c ciel -n "__fish_ciel_using_subcommand instconf" -l add-nspawn-opt -d 'Add an extra nspawn option' -r
complete -c ciel -n "__fish_ciel_using_subcommand instconf" -l remove-nspawn-opt -d 'Remove an extra nspawn option' -r
complete -c ciel -n "__fish_ciel_using_subcommand instconf" -l force-no-rollback -d 'Do not rollback instances to apply configuration'
complete -c ciel -n "__fish_ciel_using_subcommand instconf" -l unset-tmpfs-size -d 'Reset tmpfs size to default'
complete -c ciel -n "__fish_ciel_using_subcommand instconf" -l unset-output -d 'Use default output directory'
complete -c ciel -n "__fish_ciel_using_subcommand instconf" -l unset-repo -d 'Remove all extra APT repository'
complete -c ciel -n "__fish_ciel_using_subcommand instconf" -l unset-nspawn-opt -d 'Remove all extra nspawn option'
complete -c ciel -n "__fish_ciel_using_subcommand instconf" -s h -l help -d 'Print help'
complete -c ciel -n "__fish_ciel_using_subcommand config" -s m -l maintainer -d 'Maintainer information' -r
complete -c ciel -n "__fish_ciel_using_subcommand config" -l dnssec -d 'Enable DNSSEC' -r -f -a "{true\t'',false\t''}"
complete -c ciel -n "__fish_ciel_using_subcommand config" -l local-repo -d 'Enable local package repository' -r -f -a "{true\t'',false\t''}"
complete -c ciel -n "__fish_ciel_using_subcommand config" -l source-cache -d 'Enable local source caches' -r -f -a "{true\t'',false\t''}"
complete -c ciel -n "__fish_ciel_using_subcommand config" -l branch-exclusive-output -d 'Use different OUTPUT directory for branches' -r -f -a "{true\t'',false\t''}"
complete -c ciel -n "__fish_ciel_using_subcommand config" -l volatile-mount -d 'Enable volatile mount' -r -f -a "{true\t'',false\t''}"
complete -c ciel -n "__fish_ciel_using_subcommand config" -l use-apt -d 'Force to use APT' -r -f -a "{true\t'',false\t''}"
complete -c ciel -n "__fish_ciel_using_subcommand config" -l add-repo -d 'Add an extra APT repository' -r
complete -c ciel -n "__fish_ciel_using_subcommand config" -l remove-repo -d 'Remove an extra APT repository' -r
complete -c ciel -n "__fish_ciel_using_subcommand config" -l add-nspawn-opt -d 'Add an extra nspawn option' -r
complete -c ciel -n "__fish_ciel_using_subcommand config" -l remove-nspawn-opt -d 'Remove an extra nspawn option' -r
complete -c ciel -n "__fish_ciel_using_subcommand config" -l force-no-rollback -d 'Do not rollback instances to apply configuration'
complete -c ciel -n "__fish_ciel_using_subcommand config" -l unset-repo -d 'Remove all extra APT repository'
complete -c ciel -n "__fish_ciel_using_subcommand config" -l unset-nspawn-opt -d 'Remove all extra nspawn option'
complete -c ciel -n "__fish_ciel_using_subcommand config" -s h -l help -d 'Print help'
complete -c ciel -n "__fish_ciel_using_subcommand load-tree" -s h -l help -d 'Print help'
complete -c ciel -n "__fish_ciel_using_subcommand add" -l local-repo -d 'Enable local package repository' -r -f -a "{true\t'',false\t''}"
complete -c ciel -n "__fish_ciel_using_subcommand add" -l tmpfs -d 'Enable tmpfs' -r -f -a "{true\t'',false\t''}"
complete -c ciel -n "__fish_ciel_using_subcommand add" -l tmpfs-size -d 'Size of tmpfs to use, in MiB' -r
complete -c ciel -n "__fish_ciel_using_subcommand add" -l ro-tree -d 'Mount TREE as read-only' -r -f -a "{true\t'',false\t''}"
complete -c ciel -n "__fish_ciel_using_subcommand add" -l output -d 'Path to output directory' -r -F
complete -c ciel -n "__fish_ciel_using_subcommand add" -l add-repo -d 'Add an extra APT repository' -r
complete -c ciel -n "__fish_ciel_using_subcommand add" -l remove-repo -d 'Remove an extra APT repository' -r
complete -c ciel -n "__fish_ciel_using_subcommand add" -l add-nspawn-opt -d 'Add an extra nspawn option' -r
complete -c ciel -n "__fish_ciel_using_subcommand add" -l remove-nspawn-opt -d 'Remove an extra nspawn option' -r
complete -c ciel -n "__fish_ciel_using_subcommand add" -l unset-tmpfs-size -d 'Reset tmpfs size to default'
complete -c ciel -n "__fish_ciel_using_subcommand add" -l unset-output -d 'Use default output directory'
complete -c ciel -n "__fish_ciel_using_subcommand add" -l unset-repo -d 'Remove all extra APT repository'
complete -c ciel -n "__fish_ciel_using_subcommand add" -l unset-nspawn-opt -d 'Remove all extra nspawn option'
complete -c ciel -n "__fish_ciel_using_subcommand add" -s h -l help -d 'Print help'
complete -c ciel -n "__fish_ciel_using_subcommand del" -s a -l all
complete -c ciel -n "__fish_ciel_using_subcommand del" -s h -l help -d 'Print help'
complete -c ciel -n "__fish_ciel_using_subcommand mount" -s a -l all
complete -c ciel -n "__fish_ciel_using_subcommand mount" -s h -l help -d 'Print help'
complete -c ciel -n "__fish_ciel_using_subcommand boot" -s a -l all
complete -c ciel -n "__fish_ciel_using_subcommand boot" -s h -l help -d 'Print help'
complete -c ciel -n "__fish_ciel_using_subcommand stop" -s a -l all
complete -c ciel -n "__fish_ciel_using_subcommand stop" -s h -l help -d 'Print help'
complete -c ciel -n "__fish_ciel_using_subcommand down" -s a -l all
complete -c ciel -n "__fish_ciel_using_subcommand down" -s h -l help -d 'Print help'
complete -c ciel -n "__fish_ciel_using_subcommand rollback" -s a -l all
complete -c ciel -n "__fish_ciel_using_subcommand rollback" -s h -l help -d 'Print help'
complete -c ciel -n "__fish_ciel_using_subcommand commit" -s h -l help -d 'Print help'
complete -c ciel -n "__fish_ciel_using_subcommand shell" -s i -d 'Instance to be used' -r
complete -c ciel -n "__fish_ciel_using_subcommand shell" -l local-repo -d 'Enable local package repository' -r -f -a "{true\t'',false\t''}"
complete -c ciel -n "__fish_ciel_using_subcommand shell" -l tmpfs -d 'Enable tmpfs' -r -f -a "{true\t'',false\t''}"
complete -c ciel -n "__fish_ciel_using_subcommand shell" -l tmpfs-size -d 'Size of tmpfs to use, in MiB' -r
complete -c ciel -n "__fish_ciel_using_subcommand shell" -l ro-tree -d 'Mount TREE as read-only' -r -f -a "{true\t'',false\t''}"
complete -c ciel -n "__fish_ciel_using_subcommand shell" -l output -d 'Path to output directory' -r -F
complete -c ciel -n "__fish_ciel_using_subcommand shell" -l add-repo -d 'Add an extra APT repository' -r
complete -c ciel -n "__fish_ciel_using_subcommand shell" -l remove-repo -d 'Remove an extra APT repository' -r
complete -c ciel -n "__fish_ciel_using_subcommand shell" -l add-nspawn-opt -d 'Add an extra nspawn option' -r
complete -c ciel -n "__fish_ciel_using_subcommand shell" -l remove-nspawn-opt -d 'Remove an extra nspawn option' -r
complete -c ciel -n "__fish_ciel_using_subcommand shell" -l unset-tmpfs-size -d 'Reset tmpfs size to default'
complete -c ciel -n "__fish_ciel_using_subcommand shell" -l unset-output -d 'Use default output directory'
complete -c ciel -n "__fish_ciel_using_subcommand shell" -l unset-repo -d 'Remove all extra APT repository'
complete -c ciel -n "__fish_ciel_using_subcommand shell" -l unset-nspawn-opt -d 'Remove all extra nspawn option'
complete -c ciel -n "__fish_ciel_using_subcommand shell" -s h -l help -d 'Print help'
complete -c ciel -n "__fish_ciel_using_subcommand run" -s i -d 'Instance to run command in' -r
complete -c ciel -n "__fish_ciel_using_subcommand run" -s h -l help -d 'Print help'
complete -c ciel -n "__fish_ciel_using_subcommand build" -s i -d 'Instance to be used' -r
complete -c ciel -n "__fish_ciel_using_subcommand build" -l local-repo -d 'Enable local package repository' -r -f -a "{true\t'',false\t''}"
complete -c ciel -n "__fish_ciel_using_subcommand build" -l tmpfs -d 'Enable tmpfs' -r -f -a "{true\t'',false\t''}"
complete -c ciel -n "__fish_ciel_using_subcommand build" -l tmpfs-size -d 'Size of tmpfs to use, in MiB' -r
complete -c ciel -n "__fish_ciel_using_subcommand build" -l ro-tree -d 'Mount TREE as read-only' -r -f -a "{true\t'',false\t''}"
complete -c ciel -n "__fish_ciel_using_subcommand build" -l output -d 'Path to output directory' -r -F
complete -c ciel -n "__fish_ciel_using_subcommand build" -l add-repo -d 'Add an extra APT repository' -r
complete -c ciel -n "__fish_ciel_using_subcommand build" -l remove-repo -d 'Remove an extra APT repository' -r
complete -c ciel -n "__fish_ciel_using_subcommand build" -l add-nspawn-opt -d 'Add an extra nspawn option' -r
complete -c ciel -n "__fish_ciel_using_subcommand build" -l remove-nspawn-opt -d 'Remove an extra nspawn option' -r
complete -c ciel -n "__fish_ciel_using_subcommand build" -s c -l resume -d 'Resume from a Ciel checkpoint' -r
complete -c ciel -n "__fish_ciel_using_subcommand build" -l unset-tmpfs-size -d 'Reset tmpfs size to default'
complete -c ciel -n "__fish_ciel_using_subcommand build" -l unset-output -d 'Use default output directory'
complete -c ciel -n "__fish_ciel_using_subcommand build" -l unset-repo -d 'Remove all extra APT repository'
complete -c ciel -n "__fish_ciel_using_subcommand build" -l unset-nspawn-opt -d 'Remove all extra nspawn option'
complete -c ciel -n "__fish_ciel_using_subcommand build" -s g -d 'Fetch package sources only'
complete -c ciel -n "__fish_ciel_using_subcommand build" -l stage-select -d 'Select the starting point for a build'
complete -c ciel -n "__fish_ciel_using_subcommand build" -l always-discard -d 'Destory ephemeral containers if the build fails'
complete -c ciel -n "__fish_ciel_using_subcommand build" -s h -l help -d 'Print help'
complete -c ciel -n "__fish_ciel_using_subcommand repo; and not __fish_seen_subcommand_from refresh help" -s h -l help -d 'Print help'
complete -c ciel -n "__fish_ciel_using_subcommand repo; and not __fish_seen_subcommand_from refresh help" -f -a "refresh" -d 'Refresh the repository'
complete -c ciel -n "__fish_ciel_using_subcommand repo; and not __fish_seen_subcommand_from refresh help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c ciel -n "__fish_ciel_using_subcommand repo; and __fish_seen_subcommand_from refresh" -s h -l help -d 'Print help'
complete -c ciel -n "__fish_ciel_using_subcommand repo; and __fish_seen_subcommand_from help" -f -a "refresh" -d 'Refresh the repository'
complete -c ciel -n "__fish_ciel_using_subcommand repo; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c ciel -n "__fish_ciel_using_subcommand clean" -s h -l help -d 'Print help'
complete -c ciel -n "__fish_ciel_using_subcommand diagnose" -s h -l help -d 'Print help'
complete -c ciel -n "__fish_ciel_using_subcommand help; and not __fish_seen_subcommand_from version list new farewell load-os update-os instconf config load-tree add del mount boot stop down rollback commit shell run build repo clean diagnose help" -f -a "version" -d 'Display the version of CIEL!'
complete -c ciel -n "__fish_ciel_using_subcommand help; and not __fish_seen_subcommand_from version list new farewell load-os update-os instconf config load-tree add del mount boot stop down rollback commit shell run build repo clean diagnose help" -f -a "list" -d 'List all instances in the workspace'
complete -c ciel -n "__fish_ciel_using_subcommand help; and not __fish_seen_subcommand_from version list new farewell load-os update-os instconf config load-tree add del mount boot stop down rollback commit shell run build repo clean diagnose help" -f -a "new" -d 'Create a new CIEL! workspace'
complete -c ciel -n "__fish_ciel_using_subcommand help; and not __fish_seen_subcommand_from version list new farewell load-os update-os instconf config load-tree add del mount boot stop down rollback commit shell run build repo clean diagnose help" -f -a "farewell" -d 'Remove everything related to CIEL!'
complete -c ciel -n "__fish_ciel_using_subcommand help; and not __fish_seen_subcommand_from version list new farewell load-os update-os instconf config load-tree add del mount boot stop down rollback commit shell run build repo clean diagnose help" -f -a "load-os" -d 'Unpack OS tarball or fetch the latest BuildKit'
complete -c ciel -n "__fish_ciel_using_subcommand help; and not __fish_seen_subcommand_from version list new farewell load-os update-os instconf config load-tree add del mount boot stop down rollback commit shell run build repo clean diagnose help" -f -a "update-os" -d 'Update the OS in the container'
complete -c ciel -n "__fish_ciel_using_subcommand help; and not __fish_seen_subcommand_from version list new farewell load-os update-os instconf config load-tree add del mount boot stop down rollback commit shell run build repo clean diagnose help" -f -a "instconf" -d 'Configure instances'
complete -c ciel -n "__fish_ciel_using_subcommand help; and not __fish_seen_subcommand_from version list new farewell load-os update-os instconf config load-tree add del mount boot stop down rollback commit shell run build repo clean diagnose help" -f -a "config" -d 'Configure workspace'
complete -c ciel -n "__fish_ciel_using_subcommand help; and not __fish_seen_subcommand_from version list new farewell load-os update-os instconf config load-tree add del mount boot stop down rollback commit shell run build repo clean diagnose help" -f -a "load-tree" -d 'Clone abbs tree from git'
complete -c ciel -n "__fish_ciel_using_subcommand help; and not __fish_seen_subcommand_from version list new farewell load-os update-os instconf config load-tree add del mount boot stop down rollback commit shell run build repo clean diagnose help" -f -a "add" -d 'Add a new instance'
complete -c ciel -n "__fish_ciel_using_subcommand help; and not __fish_seen_subcommand_from version list new farewell load-os update-os instconf config load-tree add del mount boot stop down rollback commit shell run build repo clean diagnose help" -f -a "del" -d 'Remove one or all instance'
complete -c ciel -n "__fish_ciel_using_subcommand help; and not __fish_seen_subcommand_from version list new farewell load-os update-os instconf config load-tree add del mount boot stop down rollback commit shell run build repo clean diagnose help" -f -a "mount" -d 'Mount one or all instance'
complete -c ciel -n "__fish_ciel_using_subcommand help; and not __fish_seen_subcommand_from version list new farewell load-os update-os instconf config load-tree add del mount boot stop down rollback commit shell run build repo clean diagnose help" -f -a "boot" -d 'Start one or all instance'
complete -c ciel -n "__fish_ciel_using_subcommand help; and not __fish_seen_subcommand_from version list new farewell load-os update-os instconf config load-tree add del mount boot stop down rollback commit shell run build repo clean diagnose help" -f -a "stop" -d 'Shutdown one or all instance'
complete -c ciel -n "__fish_ciel_using_subcommand help; and not __fish_seen_subcommand_from version list new farewell load-os update-os instconf config load-tree add del mount boot stop down rollback commit shell run build repo clean diagnose help" -f -a "down" -d 'Shutdown and unmount one or all instance'
complete -c ciel -n "__fish_ciel_using_subcommand help; and not __fish_seen_subcommand_from version list new farewell load-os update-os instconf config load-tree add del mount boot stop down rollback commit shell run build repo clean diagnose help" -f -a "rollback" -d 'Rollback one or all instance'
complete -c ciel -n "__fish_ciel_using_subcommand help; and not __fish_seen_subcommand_from version list new farewell load-os update-os instconf config load-tree add del mount boot stop down rollback commit shell run build repo clean diagnose help" -f -a "commit" -d 'Commit changes onto the underlying base system'
complete -c ciel -n "__fish_ciel_using_subcommand help; and not __fish_seen_subcommand_from version list new farewell load-os update-os instconf config load-tree add del mount boot stop down rollback commit shell run build repo clean diagnose help" -f -a "shell" -d 'Start an interactive shell or run a shell command'
complete -c ciel -n "__fish_ciel_using_subcommand help; and not __fish_seen_subcommand_from version list new farewell load-os update-os instconf config load-tree add del mount boot stop down rollback commit shell run build repo clean diagnose help" -f -a "run" -d 'Run a command in the container'
complete -c ciel -n "__fish_ciel_using_subcommand help; and not __fish_seen_subcommand_from version list new farewell load-os update-os instconf config load-tree add del mount boot stop down rollback commit shell run build repo clean diagnose help" -f -a "build" -d 'Build the packages using the specified instance'
complete -c ciel -n "__fish_ciel_using_subcommand help; and not __fish_seen_subcommand_from version list new farewell load-os update-os instconf config load-tree add del mount boot stop down rollback commit shell run build repo clean diagnose help" -f -a "repo" -d 'Local repository maintenance'
complete -c ciel -n "__fish_ciel_using_subcommand help; and not __fish_seen_subcommand_from version list new farewell load-os update-os instconf config load-tree add del mount boot stop down rollback commit shell run build repo clean diagnose help" -f -a "clean" -d 'Clean all the output directories and source cache directories'
complete -c ciel -n "__fish_ciel_using_subcommand help; and not __fish_seen_subcommand_from version list new farewell load-os update-os instconf config load-tree add del mount boot stop down rollback commit shell run build repo clean diagnose help" -f -a "diagnose" -d 'Diagnose problems (hopefully)'
complete -c ciel -n "__fish_ciel_using_subcommand help; and not __fish_seen_subcommand_from version list new farewell load-os update-os instconf config load-tree add del mount boot stop down rollback commit shell run build repo clean diagnose help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c ciel -n "__fish_ciel_using_subcommand help; and __fish_seen_subcommand_from repo" -f -a "refresh" -d 'Refresh the repository'
