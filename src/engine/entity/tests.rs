use crate::engine::entity::ecs_world::ECSWorld;

#[derive(Debug, PartialEq)]
struct SimpleComponent(f32);

#[test]
fn basic_test() {
    let mut ecs = ECSWorld::new();
    let e = ecs.spawn((SimpleComponent(5.0),));

    // Get component directly;
    let Ok(component) = ecs.get::<&SimpleComponent>(e) else {
        panic!("Component should exist.");
    };
    assert_eq!(component.0, 5.0);

    // Get component through iterative query;
    let mut component_query = ecs.query::<&SimpleComponent>().into_iter();
    assert_eq!(component_query.next(), Some((e, &SimpleComponent(5.0))));
    assert_eq!(component_query.next(), None);

    // Get component through single entity query;
    let Some(component) = ecs.query_one::<&SimpleComponent>(e).get() else {
        panic!("Component should exist");
    };
    assert_eq!(component.0, 5.0);
}
