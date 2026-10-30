//! Brightness-normalised motion and presence zones.
use crate::Gray;use lumap_core::geom::Vec2;
/// Running-background motion detector.
pub struct MotionDetector{background:Vec<f32>,motion:Gray,ready:bool}
impl MotionDetector{/// Creates a detector.
pub fn new(w:u32,h:u32)->Self{Self{background:vec![0.0;w as usize*h as usize],motion:Gray::new(w,h),ready:false}}/// Updates from a camera frame while ignoring global exposure changes.
pub fn update(&mut self,f:&Gray){if f.width!=self.motion.width||f.height!=self.motion.height{*self=Self::new(f.width,f.height)}let mean=f.data.iter().map(|&v|f32::from(v)).sum::<f32>()/f.data.len().max(1)as f32;for(i,&v)in f.data.iter().enumerate(){let normal=f32::from(v)-mean+128.0;if self.ready{let d=(normal-self.background[i]).abs();self.motion.data[i]=(d*5.0).clamp(0.0,255.0)as u8;self.background[i]=self.background[i]*0.985+normal*0.015}else{self.background[i]=normal;}}self.ready=true}/// Motion amount inside a normalised polygon.
pub fn zone(&self,poly:&[Vec2],sensitivity:f64)->f64{if poly.len()<3{return 0.0}let mut sum=0.0;let mut n=0;for y in 0..self.motion.height{for x in 0..self.motion.width{let p=Vec2::new((f64::from(x)+0.5)/f64::from(self.motion.width),(f64::from(y)+0.5)/f64::from(self.motion.height));if inside(p,poly){sum+=f64::from(self.motion.get(x,y))/255.0;n+=1}}}if n==0{0.0}else{(sum/f64::from(n)*sensitivity*5.0).clamp(0.0,1.0)}}/// Debug motion image.
pub fn motion_image(&self)->Gray{self.motion.clone()}}
fn inside(p:Vec2,poly:&[Vec2])->bool{let mut yes=false;for i in 0..poly.len(){let(a,b)=(poly[i],poly[(i+1)%poly.len()]);if(a.y>p.y)!=(b.y>p.y)&&p.x<(b.x-a.x)*(p.y-a.y)/(b.y-a.y)+a.x{yes=!yes}}yes}
