function __ciel_list_instances
    if ! test -d .ciel/container/instances
        return
    end
    find .ciel/container/instances -maxdepth 1 -mindepth 1 -type d -printf '%f\tInstance\n'
end

function __ciel_list_packages
    if ! test -d TREE
        return
    end
    find "TREE/groups/" -maxdepth 1 -mindepth 1 -type f -printf 'groups/%f\n'
    if string match -q -- "*/*" "$current"
        return
    end
    find "TREE" -maxdepth 2 -mindepth 2 -type d -not -path "TREE/.git" -printf '%f\n'
end

function __ciel_list_plugins
    set ciel_path (command -v ciel)
    set ciel_plugin_dir (dirname $ciel_path)"/../libexec/ciel-plugin"
    find "$ciel_plugin_dir" -maxdepth 1 -mindepth 1 -type f -printf '%f\t-Ciel plugin-\n' | cut -d'-' -f2-
end

complete -c ciel -n "__fish_use_subcommand" -s C -d 'set the CIEL! working directory'
complete -c ciel -n "__fish_use_subcommand" -s b -l batch -d 'Batch mode, no input required'
complete -c ciel -n "__fish_use_subcommand" -s h -l help -d 'Prints help information'
complete -c ciel -n "__fish_use_subcommand" -s V -l version -d 'Prints version information'
complete -c ciel -n "__fish_use_subcommand" -f -a "version" -d 'Display the version of CIEL!'
complete -c ciel -n "__fish_use_subcommand" -f -a "init" -d 'Initialize the work directory'
complete -c ciel -n "__fish_use_subcommand" -f -a "load-os" -d 'Unpack OS tarball or fetch the latest BuildKit from the repository'
complete -c ciel -n "__fish_use_subcommand" -f -a "update-os" -d 'Update the OS in the container'
complete -c ciel -n "__fish_use_subcommand" -f -a "load-tree" -d 'Clone package tree from the link provided or AOSC OS ABBS main repository'
complete -c ciel -n "__fish_use_subcommand" -f -a "new" -d 'Create a new CIEL workspace'
complete -c ciel -n "__fish_use_subcommand" -f -a "list" -d 'List all the instances under the specified working directory'
complete -c ciel -n "__fish_use_subcommand" -f -a "add" -d 'Add a new instance'
complete -c ciel -n "__fish_use_subcommand" -f -a "del" -d 'Remove an instance'
complete -c ciel -n "__fish_use_subcommand" -f -a "shell" -d 'Start an interactive shell'
complete -c ciel -n "__fish_use_subcommand" -f -a "run" -d 'Lower-level version of \'shell\', without login environment, without sourcing ~/.bash_profile'
complete -c ciel -n "__fish_use_subcommand" -f -a "config" -d 'Configure system and toolchain for building interactively'
complete -c ciel -n "__fish_use_subcommand" -f -a "commit" -d 'Commit changes onto the shared underlying OS'
complete -c ciel -n "__fish_use_subcommand" -f -a "doctor" -d 'Diagnose problems (hopefully)'
complete -c ciel -n "__fish_use_subcommand" -f -a "build" -d 'Build the packages using the specified instance'
complete -c ciel -n "__fish_use_subcommand" -f -a "rollback" -d 'Rollback all or specified instance'
complete -c ciel -n "__fish_use_subcommand" -f -a "down" -d 'Shutdown and unmount all or one instance'
complete -c ciel -n "__fish_use_subcommand" -f -a "stop" -d 'Shuts down an instance'
complete -c ciel -n "__fish_use_subcommand" -f -a "mount" -d 'Mount all or specified instance'
complete -c ciel -n "__fish_use_subcommand" -f -a "farewell" -d 'Remove everything related to CIEL!'
complete -c ciel -n "__fish_use_subcommand" -f -a "repo" -d 'Local repository operations'
complete -c ciel -n "__fish_use_subcommand" -f -a "help" -d 'Prints this message or the help of the given subcommand(s)'
complete -c ciel -n "__fish_seen_subcommand_from version" -s h -l help -d 'Prints help information'
complete -c ciel -n "__fish_seen_subcommand_from version" -s V -l version -d 'Prints version information'
complete -c ciel -n "__fish_seen_subcommand_from init" -l upgrade -d 'Upgrade Ciel workspace from an older version'
complete -c ciel -n "__fish_seen_subcommand_from init" -s h -l help -d 'Prints help information'
complete -c ciel -n "__fish_seen_subcommand_from init" -s V -l version -d 'Prints version information'
complete -c ciel -n "__fish_seen_subcommand_from load-os" -s h -l help -d 'Prints help information'
complete -c ciel -n "__fish_seen_subcommand_from load-os" -s V -l version -d 'Prints version information'
complete -c ciel -n "__fish_seen_subcommand_from update-os" -s h -l help -d 'Prints help information'
complete -c ciel -n "__fish_seen_subcommand_from update-os" -s V -l version -d 'Prints version information'
complete -c ciel -n "__fish_seen_subcommand_from load-tree" -s h -l help -d 'Prints help information'
complete -c ciel -n "__fish_seen_subcommand_from load-tree" -s V -l version -d 'Prints version information'
complete -c ciel -n "__fish_seen_subcommand_from new" -s h -l help -d 'Prints help information'
complete -c ciel -n "__fish_seen_subcommand_from new" -s V -l version -d 'Prints version information'
complete -c ciel -n "__fish_seen_subcommand_from list" -s h -l help -d 'Prints help information'
complete -c ciel -n "__fish_seen_subcommand_from list" -s V -l version -d 'Prints version information'
complete -c ciel -n "__fish_seen_subcommand_from add" -s h -l help -d 'Prints help information'
complete -c ciel -n "__fish_seen_subcommand_from add" -s V -l version -d 'Prints version information'
complete -c ciel -n "__fish_seen_subcommand_from del" -s h -l help -d 'Prints help information'
complete -c ciel -n "__fish_seen_subcommand_from del" -s V -l version -d 'Prints version information'
complete -c ciel -n "__fish_seen_subcommand_from shell" -s i -d 'Instance to be used'
complete -c ciel -n "__fish_seen_subcommand_from shell" -s h -l help -d 'Prints help information'
complete -c ciel -n "__fish_seen_subcommand_from shell" -s V -l version -d 'Prints version information'
complete -c ciel -n "__fish_seen_subcommand_from run" -s i -d 'Instance to run command in'
complete -c ciel -n "__fish_seen_subcommand_from run" -s h -l help -d 'Prints help information'
complete -c ciel -n "__fish_seen_subcommand_from run" -s V -l version -d 'Prints version information'
complete -c ciel -n "__fish_seen_subcommand_from config" -s i -d 'Instance to be configured'
complete -c ciel -n "__fish_seen_subcommand_from config" -s g -d 'Configure base system instead of an instance'
complete -c ciel -n "__fish_seen_subcommand_from config" -s h -l help -d 'Prints help information'
complete -c ciel -n "__fish_seen_subcommand_from config" -s V -l version -d 'Prints version information'
complete -c ciel -n "__fish_seen_subcommand_from commit" -s i -d 'Instance to be committed'
complete -c ciel -n "__fish_seen_subcommand_from commit" -s h -l help -d 'Prints help information'
complete -c ciel -n "__fish_seen_subcommand_from commit" -s V -l version -d 'Prints version information'
complete -c ciel -n "__fish_seen_subcommand_from doctor" -s h -l help -d 'Prints help information'
complete -c ciel -n "__fish_seen_subcommand_from doctor" -s V -l version -d 'Prints version information'
complete -c ciel -n "__fish_seen_subcommand_from build" -s i -d 'Instance to build in'
complete -c ciel -n "__fish_seen_subcommand_from build" -s h -l help -d 'Prints help information'
complete -c ciel -n "__fish_seen_subcommand_from build" -s V -l version -d 'Prints version information'
complete -c ciel -n "__fish_seen_subcommand_from rollback" -s i -d 'Instance to be rolled back'
complete -c ciel -n "__fish_seen_subcommand_from rollback" -s h -l help -d 'Prints help information'
complete -c ciel -n "__fish_seen_subcommand_from rollback" -s V -l version -d 'Prints version information'
complete -c ciel -n "__fish_seen_subcommand_from down" -s i -d 'Instance to be un-mounted'
complete -c ciel -n "__fish_seen_subcommand_from down" -s h -l help -d 'Prints help information'
complete -c ciel -n "__fish_seen_subcommand_from down" -s V -l version -d 'Prints version information'
complete -c ciel -n "__fish_seen_subcommand_from stop" -s i -d 'Instance to be stopped'
complete -c ciel -n "__fish_seen_subcommand_from stop" -s h -l help -d 'Prints help information'
complete -c ciel -n "__fish_seen_subcommand_from stop" -s V -l version -d 'Prints version information'
complete -c ciel -n "__fish_seen_subcommand_from mount" -s i -d 'Instance to be mounted'
complete -c ciel -n "__fish_seen_subcommand_from mount" -s h -l help -d 'Prints help information'
complete -c ciel -n "__fish_seen_subcommand_from mount" -s V -l version -d 'Prints version information'
complete -c ciel -n "__fish_seen_subcommand_from farewell" -s h -l help -d 'Prints help information'
complete -c ciel -n "__fish_seen_subcommand_from farewell" -s V -l version -d 'Prints version information'
complete -c ciel -n "__fish_seen_subcommand_from repo" -s h -l help -d 'Prints help information'
complete -c ciel -n "__fish_seen_subcommand_from repo" -s V -l version -d 'Prints version information'
complete -c ciel -n "__fish_seen_subcommand_from repo" -f -a "refresh" -d 'Refresh the repository'
complete -c ciel -n "__fish_seen_subcommand_from repo" -f -a "init" -d 'Initialize the repository'
complete -c ciel -n "__fish_seen_subcommand_from repo" -f -a "deinit" -d 'Uninitialize the repository'
complete -c ciel -n "__fish_seen_subcommand_from repo" -f -a "help" -d 'Prints this message or the help of the given subcommand(s)'
complete -c ciel -n "__fish_seen_subcommand_from refresh" -s h -l help -d 'Prints help information'
complete -c ciel -n "__fish_seen_subcommand_from refresh" -s V -l version -d 'Prints version information'
complete -c ciel -n "__fish_seen_subcommand_from init" -s h -l help -d 'Prints help information'
complete -c ciel -n "__fish_seen_subcommand_from init" -s V -l version -d 'Prints version information'
complete -c ciel -n "__fish_seen_subcommand_from deinit" -s h -l help -d 'Prints help information'
complete -c ciel -n "__fish_seen_subcommand_from deinit" -s V -l version -d 'Prints version information'
complete -c ciel -n "__fish_seen_subcommand_from help" -s h -l help -d 'Prints help information'
complete -c ciel -n "__fish_seen_subcommand_from help" -s V -l version -d 'Prints version information'
complete -c ciel -n "__fish_seen_subcommand_from help" -s h -l help -d 'Prints help information'
complete -c ciel -n "__fish_seen_subcommand_from help" -s V -l version -d 'Prints version information'
# Enhanced completions
complete -xc ciel -n "__fish_seen_subcommand_from build" -a "(__ciel_list_packages)"
complete -xc ciel -n "__fish_seen_subcommand_from build" -s i -d 'Instance to build in' -a "(__ciel_list_instances)"
complete -xc ciel -n "__fish_seen_subcommand_from run" -s i -d 'Instance to run command in' -a "(__ciel_list_instances)"
complete -xc ciel -n "__fish_seen_subcommand_from config" -s i -d 'Instance to be configured' -a "(__ciel_list_instances)"
complete -xc ciel -n "__fish_seen_subcommand_from commit" -s i -d 'Instance to be committed' -a "(__ciel_list_instances)"
complete -xc ciel -n "__fish_seen_subcommand_from build" -s i -d 'Instance to build in' -a "(__ciel_list_instances)"
complete -xc ciel -n "__fish_seen_subcommand_from rollback" -s i -d 'Instance to be rolled back' -a "(__ciel_list_instances)"
complete -xc ciel -n "__fish_seen_subcommand_from down" -s i -d 'Instance to be un-mounted' -a "(__ciel_list_instances)"
complete -xc ciel -n "__fish_seen_subcommand_from stop" -s i -d 'Instance to be stopped' -a "(__ciel_list_instances)"
complete -xc ciel -n "__fish_seen_subcommand_from mount" -s i -d 'Instance to be mounted' -a "(__ciel_list_instances)"
complete -xc ciel -n "__fish_seen_subcommand_from load-os" -a "(__fish_complete_suffix tar.xz)"
complete -c ciel -a "(__ciel_list_plugins)"
