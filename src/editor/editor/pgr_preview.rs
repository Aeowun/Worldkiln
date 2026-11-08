// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use egui::{Color32, Mesh, Pos2, Rect, Sense, Ui};
use egui::epaint::Vertex;

#[derive(Clone, Copy)]
struct Projected {pos:Pos2,depth:f32,height:f32}

pub(super) fn paint_terrain(
    ui:&mut Ui,
    heights:&[f32],
    size:[u32;3],
    yaw:f32,
    zoom:f32,
    vertical_range:(f32,f32),
)->bool {
    let available=ui.available_width().min(430.0).max(200.0);
    let (rect,response)=ui.allocate_exact_size(
        egui::vec2(available,250.0),Sense::drag());
    let painter=ui.painter_at(rect);
    painter.rect_filled(rect,5.0,ui.visuals().extreme_bg_color);
    let (width,depth)=(size[0] as usize,size[2] as usize);
    if width<2 || depth<2 || heights.len()!=width*depth{return response.dragged();}
    let (low,high)=vertical_range;
    let range=(high-low).max(0.01);
    let nx=width.min(64);
    let nz=depth.min(64);
    let (sin,cos)=yaw.sin_cos();
    let mut projected=Vec::with_capacity(nx*nz);
    for z in 0..nz {
        for x in 0..nx {
            let ix=x*(width-1)/(nx-1);
            let iz=z*(depth-1)/(nz-1);
            let h=heights[iz*width+ix];
            let px=x as f32/(nx-1) as f32-0.5;
            let pz=z as f32/(nz-1) as f32-0.5;
            let py=(h-low)/range*0.56;
            let across=px*cos-pz*sin;
            let far=px*sin+pz*cos;
            let pitch=0.12f32;
            let (sin_pitch,cos_pitch)=pitch.sin_cos();
            let view_height=py*cos_pitch+far*sin_pitch;
            let view_depth=far*cos_pitch-py*sin_pitch;
            let perspective=(2.0+view_depth*0.65).max(1.1);
            let scale=zoom/perspective;
            projected.push(Projected{
                pos:Pos2::new(
                    rect.center().x+across*rect.width()*1.25*scale,
                    rect.center().y+rect.height()*0.22-
                        view_height*rect.height()*2.20*scale,
                ),
                depth:view_depth,height:(h-low)/range,
            });
        }
    }
    let mut triangles=Vec::with_capacity((nx-1)*(nz-1)*2);
    for z in 0..nz-1 {for x in 0..nx-1 {
        let a=z*nx+x;
        let b=a+1;
        let c=a+nx;
        let d=c+1;
        for vertices in [[a,b,c],[b,d,c]] {
            let depth=vertices.iter().map(|&i|projected[i].depth).sum::<f32>()/3.0;
            triangles.push((depth,vertices));
        }
    }}
    triangles.sort_by(|a,b|b.0.total_cmp(&a.0));
    let mut mesh=Mesh::default();
    for (_depth,tri) in triangles {
        let a=projected[tri[0]];
        let b=projected[tri[1]];
        let c=projected[tri[2]];
        let dx=b.pos.x-a.pos.x;
        let dy=b.pos.y-a.pos.y;
        let ex=c.pos.x-a.pos.x;
        let ey=c.pos.y-a.pos.y;
        let n=(dx*ey-dy*ex).signum();
        let slope=((a.height-b.height).abs()+(a.height-c.height).abs())
            .clamp(0.0,1.0);
        let elevation=(a.height+b.height+c.height)/3.0;
        let light=(0.78+0.17*n-0.12*slope).clamp(0.45,1.15);
        let (r,g,blue)=if elevation<0.25 {
            (92.0,150.0,93.0)
        } else if elevation<0.72 {
            (145.0,139.0,105.0)
        } else {(196.0,197.0,195.0)};
        let color=Color32::from_rgb(
            (r*light).clamp(0.0,255.0) as u8,
            (g*light).clamp(0.0,255.0) as u8,
            (blue*light).clamp(0.0,255.0) as u8
        );
        let base=mesh.vertices.len() as u32;
        for p in [a,b,c] {
            mesh.vertices.push(Vertex{pos:p.pos,uv:Pos2::ZERO,color});
        }
        mesh.add_triangle(base,base+1,base+2);
    }
    painter.add(egui::Shape::mesh(mesh));
    painter.rect_stroke(rect,5.0,ui.visuals().widgets.noninteractive.bg_stroke);
    response.dragged()
}
