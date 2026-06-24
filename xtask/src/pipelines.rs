//! Composite pipelines for the build system.
//!
//! Defines multiple "pipelines" that run specific individual steps in sequence.
use crate::{build, clippy, format, metadata, test};
use crate::{flags, WorkspaceMember};
use anyhow::Context;
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path};
use xshell::{cmd, Shell};

/// Perform a default build.
///
/// Runs the following steps in sequence:
///
/// - format
/// - build
/// - test
/// - clippy
pub fn make(sh: &Shell, flags: flags::Make) -> anyhow::Result<()> {
    let err_context = || format!("failed to run pipeline 'make' with args {flags:?}");

    if flags.clean {
        crate::cargo()
            .and_then(|cargo| cmd!(sh, "{cargo} clean").run().map_err(anyhow::Error::new))
            .with_context(err_context)?;
    }

    format::format(sh, flags::Format { check: false })
        .and_then(|_| {
            build::build(
                sh,
                flags::Build {
                    release: flags.release,
                    no_plugins: false,
                    plugins_only: false,
                    no_web: flags.no_web,
                },
            )
        })
        .and_then(|_| {
            test::test(
                sh,
                flags::Test {
                    args: vec![],
                    no_web: flags.no_web,
                },
            )
        })
        .and_then(|_| clippy::clippy(sh, flags::Clippy {}))
        .with_context(err_context)
}

/// Generate a runnable executable.
///
/// Runs the following steps in sequence:
///
/// - [`build`](build::build) (release, plugins only)
/// - [`build`](build::build) (release, without plugins)
/// - [`manpage`](build::manpage)
/// - Copy the executable to [target file](flags::Install::destination)
pub fn install(sh: &Shell, flags: flags::Install) -> anyhow::Result<()> {
    let err_context = || format!("failed to run pipeline 'install' with args {flags:?}");

    // Build and optimize plugins
    build::build(
        sh,
        flags::Build {
            release: true,
            no_plugins: false,
            plugins_only: true,
            no_web: flags.no_web,
        },
    )
    .and_then(|_| {
        // Build the main executable
        build::build(
            sh,
            flags::Build {
                release: true,
                no_plugins: true,
                plugins_only: false,
                no_web: flags.no_web,
            },
        )
    })
    .and_then(|_| {
        // Generate man page
        build::manpage(sh)
    })
    .with_context(err_context)?;

    // Copy binary to destination
    let destination = if flags.destination.is_absolute() {
        flags.destination.clone()
    } else {
        std::env::current_dir()
            .context("Can't determine current working directory")?
            .join(&flags.destination)
    };
    sh.change_dir(crate::project_root());
    sh.copy_file("target/release/zellij", &destination)
        .with_context(err_context)
}

/// Run zellij debug build.
pub fn run(sh: &Shell, mut flags: flags::Run) -> anyhow::Result<()> {
    let err_context =
        |flags: &flags::Run| format!("failed to run pipeline 'run' with args {:?}", flags);

    if flags.quick_run {
        if flags.data_dir.is_some() {
            eprintln!("cannot use '--data-dir' and '--quick-run' at the same time!");
            std::process::exit(1);
        }
        flags.data_dir.replace(crate::asset_dir());
    }

    let profile = if flags.disable_deps_optimize {
        "dev"
    } else {
        "dev-opt"
    };

    if let Some(ref data_dir) = flags.data_dir {
        let data_dir = sh.current_dir().join(data_dir);
        let features = if flags.no_web {
            "disable_automatic_asset_installation"
        } else {
            "disable_automatic_asset_installation web_server_capability"
        };

        crate::cargo()
            .and_then(|cargo| {
                cmd!(sh, "{cargo} run")
                    .args(["--package", "zellij"])
                    .arg("--no-default-features")
                    .args(["--features", features])
                    .args(["--profile", profile])
                    .args(["--", "--data-dir", &format!("{}", data_dir.display())])
                    .args(&flags.args)
                    .run()
                    .map_err(anyhow::Error::new)
            })
            .with_context(|| err_context(&flags))
    } else {
        build::build(
            sh,
            flags::Build {
                release: false,
                no_plugins: false,
                plugins_only: true,
                no_web: flags.no_web,
            },
        )
        .and_then(|_| crate::cargo())
        .and_then(|cargo| {
            if flags.no_web {
                // Use dynamic metadata approach to get the correct features
                match metadata::get_no_web_features(sh, ".")
                    .context("Failed to check web features for main crate")?
                {
                    Some(features) => {
                        let mut cmd = cmd!(sh, "{cargo} run").args(["--no-default-features"]);

                        if !features.is_empty() {
                            cmd = cmd.args(["--features", &features]);
                        }

                        cmd.args(["--profile", profile])
                            .args(["--"])
                            .args(&flags.args)
                            .run()
                            .map_err(anyhow::Error::new)
                    },
                    None => {
                        // Main crate doesn't have web_server_capability, run normally
                        cmd!(sh, "{cargo} run")
                            .args(["--profile", profile])
                            .args(["--"])
                            .args(&flags.args)
                            .run()
                            .map_err(anyhow::Error::new)
                    },
                }
            } else {
                cmd!(sh, "{cargo} run")
                    .args(["--profile", profile])
                    .args(["--"])
                    .args(&flags.args)
                    .run()
                    .map_err(anyhow::Error::new)
            }
        })
        .with_context(|| err_context(&flags))
    }
}

/// Bundle local Open Mosaic release artifacts to `target/dist`.
///
/// This includes the native `mosaic` control CLI, the Zellij-compatible `zellij`
/// workspace binary, docs, schemas, smoke scripts, an archive, and a checksum.
pub fn dist(sh: &Shell, _flags: flags::Dist) -> anyhow::Result<()> {
    let err_context = || "failed to run pipeline 'dist'";

    let project_root = crate::project_root();
    sh.change_dir(&project_root);
    if sh.path_exists("target/dist") {
        sh.remove_path("target/dist").with_context(err_context)?;
    }

    let cargo = crate::cargo().with_context(err_context)?;
    cmd!(sh, "{cargo} build --release --bin mosaic --bin zellij").run()?;

    let dist_dir = project_root.join("target/dist");
    let package_dir = dist_dir.join("open-mosaic");
    let bin_dir = package_dir.join("bin");
    let doc_dir = package_dir.join("doc");
    let schema_dir = package_dir.join("schemas");
    let script_dir = package_dir.join("scripts");
    fs::create_dir_all(&bin_dir).with_context(err_context)?;
    fs::create_dir_all(&doc_dir).with_context(err_context)?;
    fs::create_dir_all(&schema_dir).with_context(err_context)?;
    fs::create_dir_all(&script_dir).with_context(err_context)?;

    sh.copy_file("target/release/mosaic", bin_dir.join("mosaic"))
        .and_then(|_| sh.copy_file("target/release/zellij", bin_dir.join("zellij")))
        .and_then(|_| sh.copy_file("LICENSE.md", package_dir.join("LICENSE.md")))
        .and_then(|_| sh.copy_file("NOTICE.md", package_dir.join("NOTICE.md")))
        .and_then(|_| sh.copy_file("README.md", doc_dir.join("README.md")))
        .with_context(err_context)?;

    for doc in [
        "docs/DISPATCH_INTEGRATION.md",
        "docs/GETTING_STARTED.md",
        "docs/MIGRATION.md",
        "docs/MOSAIC_ADAPTERS.md",
        "docs/MOSAIC_CLI.md",
        "docs/MOSAIC_GOALS.md",
        "docs/MOSAIC_MACHINES.md",
        "docs/MOSAIC_SCHEMAS.md",
        "docs/MOSAIC_WEB.md",
        "docs/OPEN_MOSAIC.md",
        "docs/RELEASE.md",
        "docs/TMUX_FOR_AGENTS.md",
        "docs/UPSTREAM_MAINTENANCE.md",
        "docs/ZELLIJ_ARCHITECTURE_AUDIT.md",
    ] {
        copy_to_basename(sh, Path::new(doc), &doc_dir).with_context(err_context)?;
    }
    for schema in ["schemas/mosaic.control.v1.schema.json"] {
        copy_to_basename(sh, Path::new(schema), &schema_dir).with_context(err_context)?;
    }
    for script in [
        "scripts/check-upstream-hygiene.sh",
        "scripts/mosaic-workflow-smoke.sh",
        "scripts/mosaic-agent-workflow-smoke.sh",
    ] {
        copy_to_basename(sh, Path::new(script), &script_dir).with_context(err_context)?;
    }

    let host = host_triple(sh).with_context(err_context)?;
    let archive_name = format!("open-mosaic-{host}.tar.gz");
    let checksum_name = format!("{archive_name}.sha256sum");
    let archive_path = dist_dir.join(&archive_name);
    let checksum_path = dist_dir.join(&checksum_name);
    let manifest = format!(
        "name: open-mosaic\nhost: {host}\narchive: {archive_name}\nbinaries:\n  - mosaic\n  - zellij\n"
    );
    fs::write(package_dir.join("MANIFEST.txt"), manifest).with_context(err_context)?;

    cmd!(sh, "tar -C target/dist -czf {archive_path} open-mosaic").run()?;
    write_sha256sum(&archive_path, &checksum_path).with_context(err_context)?;
    println!("Open Mosaic package: {}", archive_path.display());
    println!("Open Mosaic checksum: {}", checksum_path.display());
    Ok(())
}

fn copy_to_basename(sh: &Shell, source: &Path, destination_dir: &Path) -> anyhow::Result<()> {
    let filename = source
        .file_name()
        .context("source path is missing a file name")?;
    sh.copy_file(source, destination_dir.join(filename))
        .map_err(anyhow::Error::new)
}

fn host_triple(sh: &Shell) -> anyhow::Result<String> {
    let rustc = cmd!(sh, "rustc -vV").read()?;
    rustc
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(ToOwned::to_owned)
        .context("failed to read host triple from rustc -vV")
}

fn write_sha256sum(archive_path: &Path, checksum_path: &Path) -> anyhow::Result<()> {
    let mut file = fs::File::open(archive_path)
        .with_context(|| format!("failed to open {}", archive_path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let bytes = file
            .read(&mut buffer)
            .with_context(|| format!("failed to read {}", archive_path.display()))?;
        if bytes == 0 {
            break;
        }
        hasher.update(&buffer[..bytes]);
    }
    let digest = hasher.finalize();
    let archive_name = archive_path
        .file_name()
        .and_then(|name| name.to_str())
        .context("archive path is missing a UTF-8 file name")?;
    fs::write(checksum_path, format!("{digest:x}  {archive_name}\n"))
        .with_context(|| format!("failed to write {}", checksum_path.display()))
}

/// Actions for the user to choose from to resolve publishing errors/conflicts.
enum UserAction {
    Retry,
    Abort,
    Ignore,
}

/// Make a zellij release and publish all crates.
pub fn publish(sh: &Shell, flags: flags::Publish) -> anyhow::Result<()> {
    let err_context = "failed to publish zellij";

    // Process flags
    let dry_run = if flags.dry_run {
        Some("--dry-run")
    } else {
        None
    };
    let remote = flags.git_remote.unwrap_or("origin".into());
    let registry = if let Some(ref registry) = flags.cargo_registry {
        Some(format!(
            "--registry={}",
            registry
                .clone()
                .into_string()
                .map_err(|registry| anyhow::Error::msg(format!(
                    "failed to convert '{:?}' to valid registry name",
                    registry
                )))
                .context(err_context)?
        ))
    } else {
        None
    };
    let registry = registry.as_ref();
    if flags.no_push && flags.cargo_registry.is_none() {
        anyhow::bail!("flag '--no-push' can only be used with '--cargo-registry'");
    }

    sh.change_dir(crate::project_root());
    let cargo = crate::cargo().context(err_context)?;
    let project_dir = crate::project_root();
    let manifest = sh
        .read_file(project_dir.join("Cargo.toml"))
        .context(err_context)?
        .parse::<toml::Value>()
        .context(err_context)?;
    // Version of the core crate
    let version = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .and_then(|package| package["version"].as_str())
        .context("failed to read package version from manifest")
        .context(err_context)?;

    let mut skip_build = false;
    if cmd!(sh, "git tag -l")
        .read()
        .context(err_context)?
        .contains(version)
    {
        println!();
        println!("Git tag 'v{version}' is already present.");
        println!("If this is a mistake, delete it with: git tag -d 'v{version}'");
        println!("Skip build phase and continue to publish? [y/n]");

        let stdin = std::io::stdin();
        loop {
            let mut buffer = String::new();
            stdin.read_line(&mut buffer).context(err_context)?;
            match buffer.trim_end() {
                "y" | "Y" => {
                    skip_build = true;
                    break;
                },
                "n" | "N" => {
                    skip_build = false;
                    break;
                },
                _ => {
                    println!(" --> Unknown input '{buffer}', ignoring...");
                    println!();
                    println!("Skip build phase and continue to publish? [y/n]");
                },
            }
        }
    }

    if !skip_build {
        // Clean project
        cmd!(sh, "{cargo} clean").run().context(err_context)?;

        // Build plugins
        build::build(
            sh,
            flags::Build {
                release: true,
                no_plugins: false,
                plugins_only: true,
                no_web: false,
            },
        )
        .context(err_context)?;

        // Update default config
        sh.copy_file(
            project_dir
                .join("zellij-utils")
                .join("assets")
                .join("config")
                .join("default.kdl"),
            project_dir.join("example").join("default.kdl"),
        )
        .context(err_context)?;

        // Commit changes
        cmd!(sh, "git commit -aem")
            .arg(format!("chore(release): v{}", version))
            .run()
            .context(err_context)?;

        // Tag release
        cmd!(sh, "git tag --annotate --message")
            .arg(format!("Version {}", version))
            .arg(format!("v{}", version))
            .run()
            .context(err_context)?;
    }

    let closure = || -> anyhow::Result<()> {
        // Push commit and tag
        if flags.dry_run {
            println!("Skipping push due to dry-run");
        } else if flags.no_push {
            println!("Skipping push due to no-push");
        } else {
            let branch = cmd!(sh, "git rev-parse --abbrev-ref HEAD")
                .read()
                .context(err_context)?;
            cmd!(sh, "git push --atomic {remote} {branch} v{version}")
                .run()
                .context(err_context)?;
        }

        // Publish all the crates
        for WorkspaceMember { crate_name, .. } in crate::workspace_members().iter() {
            if crate_name.contains("plugin") || crate_name.contains("xtask") {
                continue;
            }

            let _pd = sh.push_dir(project_dir.join(crate_name));
            loop {
                let msg = format!(">> Publishing '{crate_name}'");
                crate::status(&msg);
                println!("{}", msg);

                let more_args = match *crate_name {
                    // This is needed for zellij to pick up the plugins from the assets included in
                    // the released zellij-utils binary
                    "." => Some("--no-default-features"),
                    _ => None,
                };

                if let Err(err) = cmd!(
                    sh,
                    "{cargo} publish --locked {registry...} {more_args...} {dry_run...}"
                )
                .run()
                .context(err_context)
                {
                    println!();
                    println!("Publishing crate '{crate_name}' failed with error:");
                    println!("{:?}", err);
                    println!();
                    println!("Please choose what to do: [r]etry/[a]bort/[i]gnore");

                    let stdin = std::io::stdin();
                    let action;

                    loop {
                        let mut buffer = String::new();
                        stdin.read_line(&mut buffer).context(err_context)?;
                        match buffer.trim_end() {
                            "r" | "R" => {
                                action = UserAction::Retry;
                                break;
                            },
                            "a" | "A" => {
                                action = UserAction::Abort;
                                break;
                            },
                            "i" | "I" => {
                                action = UserAction::Ignore;
                                break;
                            },
                            _ => {
                                println!(" --> Unknown input '{buffer}', ignoring...");
                                println!();
                                println!("Please choose what to do: [r]etry/[a]bort/[i]gnore");
                            },
                        }
                    }

                    match action {
                        UserAction::Retry => continue,
                        UserAction::Ignore => break,
                        UserAction::Abort => {
                            eprintln!("Aborting publish for crate '{crate_name}'");
                            return Err::<(), _>(err);
                        },
                    }
                } else {
                    // publish successful, continue to next crate
                    break;
                }
            }
        }

        println!();
        println!(" +-----------------------------------------------+");
        println!(" | PRAISE THE DEVS, WE HAVE A NEW ZELLIJ RELEASE |");
        println!(" +-----------------------------------------------+");
        Ok(())
    };

    // We run this in a closure so that a failure in any of the commands doesn't abort the whole
    // program. When dry-running we need to undo the release commit first!
    let result = closure();

    if flags.dry_run && !skip_build {
        cmd!(sh, "git reset --hard HEAD~1")
            .run()
            .context(err_context)?;
    }

    result
}
