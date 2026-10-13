use glam::{Mat4, Quat, Vec3};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }
}

impl Transform {
    pub fn to_matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }

    pub fn lerp(&self, other: &Self, t: f32) -> Self {
        Self {
            translation: self.translation.lerp(other.translation, t),
            rotation: self.rotation.slerp(other.rotation, t),
            scale: self.scale.lerp(other.scale, t),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Joint {
    pub name: String,
    pub parent_index: Option<usize>,
    pub local_transform: Transform,
}

#[derive(Clone, Debug)]
pub struct Skeleton {
    pub joints: Vec<Joint>,
}

impl Skeleton {
    pub fn new(joints: Vec<Joint>) -> Self {
        Self { joints }
    }

    pub fn joint_index(&self, name: &str) -> Option<usize> {
        self.joints
            .iter()
            .position(|joint| joint.name.eq_ignore_ascii_case(name))
    }

    pub fn evaluate_pose(&self, local_transforms: &[Transform]) -> EvaluatedPose {
        assert_eq!(local_transforms.len(), self.joints.len());

        let mut global_matrices = Vec::with_capacity(self.joints.len());

        for (i, joint) in self.joints.iter().enumerate() {
            let local_matrix = local_transforms[i].to_matrix();

            let global_matrix = if let Some(parent_idx) = joint.parent_index {
                global_matrices[parent_idx] * local_matrix
            } else {
                local_matrix
            };

            global_matrices.push(global_matrix);
        }

        EvaluatedPose {
            matrices: global_matrices,
        }
    }
}

#[derive(Clone, Debug)]
pub struct EvaluatedPose {
    pub matrices: Vec<Mat4>,
}
