// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use std::ffi::CString;
use std::ptr;

pub fn create_program(vertex_source: &str, fragment_source: &str) -> u32 {
    let vertex_shader = compile_shader(gl::VERTEX_SHADER, vertex_source, "vertex");
    let fragment_shader = compile_shader(gl::FRAGMENT_SHADER, fragment_source, "fragment");

    unsafe {
        let program = gl::CreateProgram();

        gl::AttachShader(program, vertex_shader);
        gl::AttachShader(program, fragment_shader);
        gl::LinkProgram(program);

        let mut success = 0;
        gl::GetProgramiv(program, gl::LINK_STATUS, &mut success);

        if success == 0 {
            let log = get_program_log(program);

            gl::DeleteShader(vertex_shader);
            gl::DeleteShader(fragment_shader);
            gl::DeleteProgram(program);

            panic!("OpenGL shader program link failed:\n{}", log);
        }

        gl::DeleteShader(vertex_shader);
        gl::DeleteShader(fragment_shader);

        program
    }
}

fn compile_shader(kind: u32, source: &str, name: &str) -> u32 {
    let source = CString::new(source)
        .unwrap_or_else(|_| panic!("OpenGL {} shader contains an embedded NUL byte.", name));

    unsafe {
        let shader = gl::CreateShader(kind);

        gl::ShaderSource(shader, 1, &source.as_ptr(), ptr::null());

        gl::CompileShader(shader);

        let mut success = 0;
        gl::GetShaderiv(shader, gl::COMPILE_STATUS, &mut success);

        if success == 0 {
            let log = get_shader_log(shader);

            gl::DeleteShader(shader);

            panic!("OpenGL {} shader compilation failed:\n{}", name, log);
        }

        shader
    }
}

fn get_shader_log(shader: u32) -> String {
    unsafe {
        let mut length = 0;
        gl::GetShaderiv(shader, gl::INFO_LOG_LENGTH, &mut length);

        if length <= 1 {
            return String::new();
        }

        let mut buffer = vec![0u8; length as usize];

        gl::GetShaderInfoLog(
            shader,
            length,
            ptr::null_mut(),
            buffer.as_mut_ptr() as *mut i8,
        );

        String::from_utf8_lossy(&buffer)
            .trim_end_matches('\0')
            .to_string()
    }
}

fn get_program_log(program: u32) -> String {
    unsafe {
        let mut length = 0;
        gl::GetProgramiv(program, gl::INFO_LOG_LENGTH, &mut length);

        if length <= 1 {
            return String::new();
        }

        let mut buffer = vec![0u8; length as usize];

        gl::GetProgramInfoLog(
            program,
            length,
            ptr::null_mut(),
            buffer.as_mut_ptr() as *mut i8,
        );

        String::from_utf8_lossy(&buffer)
            .trim_end_matches('\0')
            .to_string()
    }
}

pub fn try_create_program(vertex_source: &str, fragment_source: &str) -> Result<u32, String> {
    fn checked_shader(kind: u32, source: &str) -> Result<u32, String> {
        let bytes =
            CString::new(source).map_err(|_| "Material shader contains a NUL byte".to_owned())?;
        unsafe {
            let shader = gl::CreateShader(kind);
            if shader == 0 {
                return Err("OpenGL could not allocate Material shader".into());
            }
            gl::ShaderSource(shader, 1, &bytes.as_ptr(), ptr::null());
            gl::CompileShader(shader);
            let mut status = 0;
            gl::GetShaderiv(shader, gl::COMPILE_STATUS, &mut status);
            if status == 0 {
                let detail = get_shader_log(shader);
                gl::DeleteShader(shader);
                return Err(format!("Material GLSL error: {detail}"));
            }
            Ok(shader)
        }
    }
    let vertex = checked_shader(gl::VERTEX_SHADER, vertex_source)?;
    let fragment = match checked_shader(gl::FRAGMENT_SHADER, fragment_source) {
        Ok(fragment) => fragment,
        Err(err) => {
            unsafe {
                gl::DeleteShader(vertex);
            }
            return Err(err);
        }
    };
    unsafe {
        let program = gl::CreateProgram();
        if program == 0 {
            gl::DeleteShader(vertex);
            gl::DeleteShader(fragment);
            return Err("OpenGL could not allocate Material program".into());
        }
        gl::AttachShader(program, vertex);
        gl::AttachShader(program, fragment);
        gl::LinkProgram(program);
        gl::DeleteShader(vertex);
        gl::DeleteShader(fragment);
        let mut status = 0;
        gl::GetProgramiv(program, gl::LINK_STATUS, &mut status);
        if status == 0 {
            let detail = get_program_log(program);
            gl::DeleteProgram(program);
            return Err(format!("Material program link error: {detail}"));
        }
        Ok(program)
    }
}
