use std::ffi::CStr;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GraphicsCapabilities {
    pub vendor: String,
    pub renderer: String,
    pub version: String,
    pub shading_language_version: String,
    pub major_version: i32,
    pub minor_version: i32,
    pub max_texture_size: i32,
    pub max_color_attachments: i32,
    pub max_draw_buffers: i32,
    pub max_shader_storage_buffer_bindings: i32,
    pub compute_shader: bool,
    pub shader_storage_buffers: bool,
    pub timer_queries: bool,
}

impl GraphicsCapabilities {
    pub fn detect() -> Self {
        unsafe {
            let mut capabilities = Self {
                vendor: gl_string(gl::VENDOR),
                renderer: gl_string(gl::RENDERER),
                version: gl_string(gl::VERSION),
                shading_language_version: gl_string(gl::SHADING_LANGUAGE_VERSION),
                ..Self::default()
            };

            gl::GetIntegerv(gl::MAJOR_VERSION, &mut capabilities.major_version);
            gl::GetIntegerv(gl::MINOR_VERSION, &mut capabilities.minor_version);
            gl::GetIntegerv(gl::MAX_TEXTURE_SIZE, &mut capabilities.max_texture_size);
            gl::GetIntegerv(
                gl::MAX_COLOR_ATTACHMENTS,
                &mut capabilities.max_color_attachments,
            );
            gl::GetIntegerv(gl::MAX_DRAW_BUFFERS, &mut capabilities.max_draw_buffers);

            let open_gl_43 =
                supports_version(capabilities.major_version, capabilities.minor_version, 4, 3);
            capabilities.compute_shader = open_gl_43 && gl::DispatchCompute::is_loaded();
            capabilities.shader_storage_buffers = open_gl_43
                && gl::BindBufferBase::is_loaded()
                && gl::ShaderStorageBlockBinding::is_loaded();
            if capabilities.shader_storage_buffers {
                gl::GetIntegerv(
                    gl::MAX_SHADER_STORAGE_BUFFER_BINDINGS,
                    &mut capabilities.max_shader_storage_buffer_bindings,
                );
            }
            capabilities.timer_queries = gl::QueryCounter::is_loaded()
                && gl::GetQueryObjectui64v::is_loaded()
                && gl::GenQueries::is_loaded();
            capabilities
        }
    }

    pub fn supports_compute_gi_research(&self) -> bool {
        self.compute_shader
            && self.shader_storage_buffers
            && self.max_shader_storage_buffer_bindings >= 4
            && self.max_color_attachments >= 3
            && self.max_draw_buffers >= 3
    }

    pub fn log(&self) {
        eprintln!(
            "AEO renderer: GPU='{}' vendor='{}' OpenGL='{}' GLSL='{}'",
            self.renderer, self.vendor, self.version, self.shading_language_version
        );
        eprintln!(
            "AEO renderer: max_texture={} color_attachments={} draw_buffers={} SSBO_bindings={} compute={} timer_queries={} GI_research_supported={}",
            self.max_texture_size,
            self.max_color_attachments,
            self.max_draw_buffers,
            self.max_shader_storage_buffer_bindings,
            self.compute_shader && self.shader_storage_buffers,
            self.timer_queries,
            self.supports_compute_gi_research()
        );
    }
}

fn supports_version(major: i32, minor: i32, required_major: i32, required_minor: i32) -> bool {
    major > required_major || (major == required_major && minor >= required_minor)
}

unsafe fn gl_string(name: u32) -> String {
    let value = unsafe { gl::GetString(name) };
    if value.is_null() {
        return "Unavailable".to_string();
    }
    unsafe { CStr::from_ptr(value.cast()) }
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_requirement_handles_major_and_minor() {
        assert!(!supports_version(3, 3, 4, 3));
        assert!(!supports_version(4, 2, 4, 3));
        assert!(supports_version(4, 3, 4, 3));
        assert!(supports_version(4, 6, 4, 3));
        assert!(supports_version(5, 0, 4, 3));
    }

    #[test]
    fn gi_support_requires_every_resource_class() {
        let mut capabilities = GraphicsCapabilities {
            compute_shader: true,
            shader_storage_buffers: true,
            max_shader_storage_buffer_bindings: 8,
            max_color_attachments: 8,
            max_draw_buffers: 8,
            ..GraphicsCapabilities::default()
        };
        assert!(capabilities.supports_compute_gi_research());
        capabilities.max_draw_buffers = 2;
        assert!(!capabilities.supports_compute_gi_research());
    }
}
