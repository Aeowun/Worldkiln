use super::Renderer;

impl Renderer {
    pub fn render_home(&self) {
        unsafe {
            gl::Viewport(0, 0, self.width as i32, self.height as i32);

            gl::Disable(gl::SCISSOR_TEST);
            gl::Disable(gl::DEPTH_TEST);

            gl::DepthMask(gl::TRUE);

            gl::ClearColor(1.0 / 255.0, 1.0 / 255.0, 2.0 / 255.0, 1.0);

            gl::Clear(gl::COLOR_BUFFER_BIT | gl::DEPTH_BUFFER_BIT);
        }
    }
}
