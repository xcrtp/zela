// bounds.rs — **局部**包围盒（B3：local only）
//
// # 为什么只做 local
//
// 世界包围盒要沿 `Parent` 链累乘变换，而带 rotation 的变换后 AABB 是**保守包围盒**
// 不是最小包围盒。把它和 local 混进同一个 trait，将来 hit-test 会出现"命中了但
// 图形其实不在那"—— 正是项目反复杀的静默误判。世界侧等父子变换接线后另开
// `world_bounds()`，并显式注明它是保守的。
//
// # 空 / 退化
//
// 一律 `Option` 贯穿到底：空路径没有包围盒就是 `None`，**不是** `Rect2D::default()`。
//
// # 与 `Reveal::current_pos` 的同一采样
//
// 两者都走 `Path2D::flatten(MORPH_TOLERANCE)`；`bounds_contain_the_reveal_tip` 用例
// 把"笔尖是否落在包围盒内"钉住 —— 采样一旦分叉，本用例先红。

use ze_notion::{Path2D, Rect2D, Vec2};
use ze_scene_contract::ObjectId;

use crate::object::{Arrow, Circle, Line, Path, Rect};
use crate::scene_write::{read_component, SceneRead};

/// 能给出**局部**包围盒的图元（`None` = 空 / 退化几何）。
pub trait Bounds2D {
    fn local_bounds(&self) -> Option<Rect2D>;
}

impl Bounds2D for Circle {
    fn local_bounds(&self) -> Option<Rect2D> {
        let center = self.as_primitive().center;
        let radius = self.as_primitive().radius;
        if !radius.is_finite() || radius < 0.0 {
            return None;
        }
        Some(Rect2D::from_center(center, Vec2::splat(radius)))
    }
}

impl Bounds2D for Path {
    fn local_bounds(&self) -> Option<Rect2D> {
        self.as_primitive().compute_bounds()
    }
}

impl Bounds2D for Line {
    fn local_bounds(&self) -> Option<Rect2D> {
        let (a, b) = (self.a(), self.b());
        point_pair_bounds(a, b)
    }
}

impl Bounds2D for Arrow {
    fn local_bounds(&self) -> Option<Rect2D> {
        // 只取 from / to 两端（箭头头部宽度是**装饰**，不改变"图形在哪"）。
        let primitive = self.as_primitive();
        point_pair_bounds(primitive.from, primitive.to)
    }
}

impl Bounds2D for Rect {
    fn local_bounds(&self) -> Option<Rect2D> {
        let rect = *self.as_primitive();
        (rect.min.is_finite() && rect.max.is_finite()).then_some(rect)
    }
}

impl Bounds2D for Path2D {
    fn local_bounds(&self) -> Option<Rect2D> {
        self.compute_bounds()
    }
}

fn point_pair_bounds(a: Vec2, b: Vec2) -> Option<Rect2D> {
    if !a.is_finite() || !b.is_finite() {
        return None;
    }
    Some(Rect2D::new(a.min(b), a.max(b)))
}

/// **对象级聚合**：把该对象上所有已知图元的局部包围盒取并集。
///
/// 读取顺序**固定**（Circle → Path → Line → Arrow → Rect），不依赖任何容器迭代序；
/// 一个图元都没有 ⇒ `None`（不是零矩形）。
pub fn object_bounds(scene: &dyn SceneRead, id: ObjectId) -> Option<Rect2D> {
    let mut out: Option<Rect2D> = None;
    let mut merge = |bounds: Option<Rect2D>| {
        if let Some(bounds) = bounds {
            out = Some(match out {
                Some(current) => current.union(&bounds),
                None => bounds,
            });
        }
    };
    merge(read_component::<Circle, _>(scene, id, |c| c.local_bounds()).flatten());
    merge(read_component::<Path, _>(scene, id, |p| p.local_bounds()).flatten());
    merge(read_component::<Line, _>(scene, id, |l| l.local_bounds()).flatten());
    merge(read_component::<Arrow, _>(scene, id, |a| a.local_bounds()).flatten());
    merge(read_component::<Rect, _>(scene, id, |r| r.local_bounds()).flatten());
    merge(read_component::<crate::components::Reveal, _>(scene, id, |r| {
        r.path.compute_bounds()
    })
    .flatten());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Reveal;
    use ze_notion::Path2D;

    fn line_path() -> Path2D {
        Path2D::from_polyline(
            &[Vec2::new(0.0, 0.0), Vec2::new(4.0, 2.0)],
            false,
        )
    }

    #[test]
    fn each_primitive_has_a_local_bounds() {
        let bounds = Circle::new((1.0, 2.0), 3.0).local_bounds().expect("圆有包围盒");
        assert_eq!(bounds.min, Vec2::new(-2.0, -1.0));
        assert_eq!(bounds.max, Vec2::new(4.0, 5.0));

        let line = Line::new((0.0, 0.0), (2.0, -1.0)).local_bounds().expect("线有包围盒");
        assert_eq!(line.min, Vec2::new(0.0, -1.0));
        assert_eq!(line.max, Vec2::new(2.0, 0.0));

        let arrow = Arrow::new((0.0, 0.0), (1.0, 2.0), 0.2, 0.2)
            .local_bounds()
            .expect("箭头有包围盒");
        assert_eq!(arrow.max, Vec2::new(1.0, 2.0));

        let rect = Rect::new((1.0, 1.0), (3.0, 4.0));
        assert_eq!(rect.local_bounds(), Some(*rect.as_primitive()));

        assert!(Path::new(Path2D::new()).local_bounds().is_none(), "空路径 ⇒ None");
    }

    /// 迷你场景：只挂一个圆（`object_bounds` 的聚合 / 空集用例够用）。
    #[derive(Default)]
    struct MockScene {
        circle: Option<Circle>,
    }

    impl SceneRead for MockScene {
        fn with_component_ref(
            &self,
            _id: ObjectId,
            type_id: std::any::TypeId,
            f: &mut dyn FnMut(&dyn std::any::Any),
        ) -> bool {
            if type_id == std::any::TypeId::of::<Circle>() {
                if let Some(circle) = &self.circle {
                    f(circle);
                    return true;
                }
            }
            false
        }
    }

    #[test]
    fn object_bounds_aggregates_and_is_none_when_empty() {
        let id = ObjectId::new(1, 1);
        assert!(
            object_bounds(&MockScene::default(), id).is_none(),
            "没有图元 ⇒ None（不是零矩形）"
        );

        let scene = MockScene {
            circle: Some(Circle::new((2.0, 3.0), 1.0)),
        };
        let bounds = object_bounds(&scene, id).expect("圆有包围盒");
        assert_eq!(bounds.min, Vec2::new(1.0, 2.0));
        assert_eq!(bounds.max, Vec2::new(3.0, 4.0));
    }

    /// **同一采样**：`Reveal::current_pos` 与包围盒都按 `flatten(MORPH_TOLERANCE)`
    /// 取点 ⇒ 笔尖必须落在包围盒内（含边界）。采样分叉 ⇒ 本用例 RED。
    #[test]
    fn bounds_contain_the_reveal_tip() {
        let path = line_path();
        let bounds = path.compute_bounds().expect("有几何");
        for end in [0.0, 0.25, 0.5, 1.0] {
            let mut reveal = Reveal::new(path.clone());
            reveal.end = end;
            let tip = reveal.current_pos();
            assert!(
                bounds.contains(tip),
                "end = {end} 的笔尖 {tip:?} 必须落在 {bounds:?} 内"
            );
        }
    }
}
