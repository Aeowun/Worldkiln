use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetPlatform {
    #[default]
    Windows,
    Linux,
    Macos,
    Android,
    Ios,
    Xbox,
    Web,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildStage {
    Build,
    Cook,
    Stage,
    Package,
    Deploy,
    Run,
}

impl BuildStage {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Build => "Build",
            Self::Cook => "Cook",
            Self::Stage => "Stage",
            Self::Package => "Package",
            Self::Deploy => "Deploy",
            Self::Run => "Run",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PlatformCapabilities {
    pub platform: TargetPlatform,
    pub name: &'static str,
    pub architecture: &'static str,
    pub output_formats: &'static [&'static str],
    pub native_adapter: bool,
    pub development_runtime: bool,
    pub sdk_requirement: &'static str,
}

impl TargetPlatform {
    pub const ALL: [TargetPlatform; 7] = [
        Self::Windows,
        Self::Linux,
        Self::Macos,
        Self::Android,
        Self::Ios,
        Self::Xbox,
        Self::Web,
    ];

    pub const fn capabilities(self) -> PlatformCapabilities {
        match self {
            Self::Windows => PlatformCapabilities {
                platform: self,
                name: "Windows",
                architecture: "x86_64",
                output_formats: &["folder", "zip"],
                native_adapter: cfg!(windows),
                development_runtime: true,
                sdk_requirement: "Windows native toolchain",
            },
            Self::Linux => PlatformCapabilities {
                platform: self,
                name: "Linux",
                architecture: "x86_64 / aarch64",
                output_formats: &[],
                native_adapter: false,
                development_runtime: false,
                sdk_requirement: "Linux native toolchain and portable runtime",
            },
            Self::Macos => PlatformCapabilities {
                platform: self,
                name: "macOS",
                architecture: "aarch64 / x86_64",
                output_formats: &[],
                native_adapter: false,
                development_runtime: false,
                sdk_requirement: "macOS SDK and native runtime",
            },
            Self::Android => PlatformCapabilities {
                platform: self,
                name: "Android",
                architecture: "aarch64",
                output_formats: &[],
                native_adapter: false,
                development_runtime: false,
                sdk_requirement: "Android SDK / NDK and mobile runtime",
            },
            Self::Ios => PlatformCapabilities {
                platform: self,
                name: "iOS / iPadOS",
                architecture: "aarch64",
                output_formats: &[],
                native_adapter: false,
                development_runtime: false,
                sdk_requirement: "Apple SDK, provisioning and mobile runtime",
            },
            Self::Xbox => PlatformCapabilities {
                platform: self,
                name: "Xbox Series X|S",
                architecture: "console",
                output_formats: &[],
                native_adapter: false,
                development_runtime: false,
                sdk_requirement: "Authorized Xbox GDK and console runtime",
            },
            Self::Web => PlatformCapabilities {
                platform: self,
                name: "Web",
                architecture: "wasm32",
                output_formats: &[],
                native_adapter: false,
                development_runtime: false,
                sdk_requirement: "Web runtime and browser graphics backend",
            },
        }
    }

    pub fn ensure_native_adapter(self) -> Result<(), String> {
        let desc = self.capabilities();
        if !desc.native_adapter {
            return Err(format!(
                "{} game exports are unavailable: {}.",
                desc.name, desc.sdk_requirement
            ));
        }
        Ok(())
    }
}

pub trait GamePlatformAdapter {
    fn target(&self) -> TargetPlatform;

    fn build(
        &self,
        project: &std::path::Path,
        options: &crate::engine::build::GameBuildOptions,
        cancel: Option<&std::sync::atomic::AtomicBool>,
        on_stage: &mut dyn FnMut(BuildStage),
    ) -> Result<crate::engine::build::BuildResult, String>;
}
