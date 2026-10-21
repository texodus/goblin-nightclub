use goblin_nightclub_core::entity::*;
use goblin_nightclub_core::layer::Layer;
use goblin_nightclub_core::rng::Rng;

#[test]
fn npcs_spawn_clear_and_never_enter_walls() {
    for seed in 0..20 {
        let spawn = [165.0, 105.0];
        let level = Layer::soup(seed, 32, 20);
        let mut rng = Rng::new(seed ^ 0x9E37_79B9);
        let mut npcs = Entity::spawn_npcs(&level, &mut rng, 4, 0, spawn, 40.0);
        assert_eq!(npcs.len(), 4);
        for npc in &npcs {
            let c = npc.center();
            assert!((c[0] - spawn[0]).hypot(c[1] - spawn[1]) >= 40.0);
        }
        for _ in 0..2000 {
            for npc in &mut npcs {
                npc.wander(&level, &mut rng, 1.0 / 60.0);
                assert!(!level.collides(npc.pos, npc.size), "seed {seed}");
            }
        }
    }
}
