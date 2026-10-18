//! Multi-node simulations (docs/sync-protocol.md section 11).

use perch_store::Store;
use perch_sync::sim::Sim;

const STEPS: u64 = 5_000_000;

/// Publishes `n` records from random authors at random peers. An author
/// submits to a peer that already holds its latest record, as a real client
/// does after syncing; otherwise to its home peer.
fn publish_random(sim: &mut Sim, n: usize, interleave: bool) {
    let nodes = sim.nodes.len();
    let authors = sim.authors.len();
    for _ in 0..n {
        let a = sim.rng.below(authors);
        let mut node = sim.rng.below(nodes);
        let au = &sim.authors[a];
        let last = au.prev;
        let head = sim.nodes[node].store.head(&au.key.author()).and_then(|h| h.id);
        if head != last {
            node = a % nodes;
        }
        sim.publish(node, a);
        if interleave {
            let k = sim.rng.below(200) as u64;
            sim.run(k);
        }
    }
}

fn converge(sim: &mut Sim, peers: &[usize]) -> perch_sync::sim::NodeState {
    sim.run(STEPS);
    match sim.check_converged(peers) {
        Ok(s) => s,
        Err(e) => panic!("not converged: {e}"),
    }
}

fn topology_case(n: usize, seed: u64, topo: &str) {
    let mut sim = Sim::new(n, seed);
    sim.dup_push = 0.05;
    sim.add_authors(6, seed as u8);
    match topo {
        "line" => sim.line(),
        "ring" => sim.ring(),
        "star" => sim.star(),
        _ => sim.random_graph(n / 2),
    }
    publish_random(&mut sim, 60, true);
    let state = { let all = sim.all(); converge(&mut sim, &all) };
    let total: u64 = state.heads.iter().map(|h| h.1).sum();
    assert_eq!(total, 60, "{topo}: all records everywhere");
    for node in &sim.nodes {
        for s in node.hub.sessions() {
            assert_eq!(s.score(), 0, "honest peers never score each other");
        }
    }
}

#[test]
fn topologies_converge() {
    for (i, topo) in ["line", "ring", "star", "random"].iter().enumerate() {
        for n in [2, 5, 12] {
            topology_case(n, 100 + i as u64 * 10 + n as u64, topo);
        }
    }
}

#[test]
fn fifty_nodes_random_graph() {
    topology_case(50, 4242, "random");
}

#[test]
fn late_joiners_catch_up() {
    let mut sim = Sim::new(6, 7);
    sim.add_authors(4, 7);
    sim.random_graph(3);
    publish_random(&mut sim, 40, true);
    sim.run(STEPS);
    let a = sim.add_node();
    let b = sim.add_node();
    sim.connect(a, 2);
    publish_random(&mut sim, 10, true);
    sim.connect(b, a);
    let state = { let all = sim.all(); converge(&mut sim, &all) };
    assert_eq!(state.heads.iter().map(|h| h.1).sum::<u64>(), 50);
}

#[test]
fn partitions_heal() {
    let mut sim = Sim::new(10, 11);
    sim.add_authors(8, 11);
    sim.ring();
    publish_random(&mut sim, 20, true);
    sim.run(STEPS);
    // Cut the ring into {0..4} and {5..9}.
    sim.disconnect(4, 5);
    sim.disconnect(9, 0);
    // Authors 0..3 publish on side A, 4..7 on side B.
    for i in 0..30 {
        let a = i % 8;
        let node = if a < 4 { a % 5 } else { 5 + a % 5 };
        let head = sim.nodes[node].store.head(&sim.authors[a].key.author()).and_then(|h| h.id);
        if head == sim.authors[a].prev {
            sim.publish(node, a);
        }
        sim.run(50);
    }
    sim.run(STEPS);
    assert!(sim.check_converged(&[0, 1, 2, 3, 4]).is_ok());
    assert!(sim.check_converged(&[5, 6, 7, 8, 9]).is_ok());
    assert!(sim.check_converged(&sim.all()).is_err(), "sides differ while partitioned");
    sim.connect(4, 5);
    sim.connect(0, 9);
    { let all = sim.all(); converge(&mut sim, &all) };
}

#[test]
fn equivocation_yields_evidence_everywhere() {
    for seed in 0..5u64 {
        let mut sim = Sim::new(12, 300 + seed);
        sim.add_authors(3, 3);
        sim.random_graph(6);
        for _ in 0..3 {
            sim.publish(0, 0);
        }
        publish_random(&mut sim, 15, true);
        sim.run(STEPS);
        // Author 0 equivocates at its next seq, to two distant peers.
        let (x, y) = sim.equivocate(0, 1, 11);
        let state = { let all = sim.all(); converge(&mut sim, &all) };
        let author = sim.authors[0].key.author();
        let pair = state.pairs.iter().find(|(a, _)| *a == author).map(|(_, ids)| *ids).expect("pair everywhere");
        let mut want = [x.id(), y.id()];
        want.sort();
        assert_eq!(pair, want);
        for n in &sim.nodes {
            assert_eq!(n.store.fork_seq(&author), Some(x.seq()));
            assert_eq!(n.store.head(&author).unwrap().seq, x.seq() - 1);
        }
    }
}

#[test]
fn equivocation_with_branches_converges() {
    // Each side keeps extending its branch before the networks meet.
    let mut sim = Sim::new(8, 77);
    sim.add_authors(2, 9);
    sim.line();
    for _ in 0..4 {
        sim.publish(0, 0);
    }
    sim.run(STEPS);
    sim.disconnect(3, 4);
    let base_seq = sim.authors[0].seq;
    let base_prev = sim.authors[0].prev;
    // Branch X at peer 0.
    for _ in 0..5 {
        sim.publish(0, 0);
    }
    // Branch Y at peer 7, from the same base.
    sim.authors[0].seq = base_seq;
    sim.authors[0].prev = base_prev;
    sim.authors[0].ts += 7;
    for _ in 0..5 {
        sim.publish(7, 0);
    }
    sim.run(STEPS);
    sim.connect(3, 4);
    let state = { let all = sim.all(); converge(&mut sim, &all) };
    let author = sim.authors[0].key.author();
    assert!(state.pairs.iter().any(|(a, _)| *a == author));
    assert_eq!(sim.nodes[0].store.fork_seq(&author), Some(base_seq + 1));
}

#[test]
fn body_stripping_peer_cannot_hide_content() {
    // Peer 1 strips bodies. Peer 2 first syncs only through it, then meets peer 0.
    let mut sim = Sim::new(4, 21);
    sim.add_authors(3, 21);
    sim.nodes[1].strip_bodies = true;
    publish_random_at(&mut sim, 0, 30);
    sim.connect(0, 1);
    sim.run(STEPS);
    sim.connect(1, 2);
    sim.run(STEPS);
    assert_eq!(sim.nodes[2].store.bodyless_count(), 30, "peer 2 holds headers only");
    sim.connect(2, 0);
    sim.connect(3, 1);
    // Live gossip through the stripper while peer 3 is still behind it.
    publish_random_at(&mut sim, 0, 10);
    sim.run(STEPS);
    sim.connect(3, 2);
    let state = {
        let peers = [0, 2, 3];
        sim.run(STEPS);
        sim.check_converged(&peers).unwrap_or_else(|e| panic!("{e}"))
    };
    assert!(state.bodyless.is_empty(), "honest peers have every body");
    assert!(sim.stats.by_type.get("want_bodies").is_some_and(|x| x.0 > 0));
}

fn publish_random_at(sim: &mut Sim, node: usize, n: usize) {
    for _ in 0..n {
        let a = sim.rng.below(sim.authors.len());
        sim.publish(node, a);
    }
}

#[test]
fn deterministic_for_a_seed() {
    let run = |seed| {
        let mut sim = Sim::new(8, seed);
        sim.add_authors(4, 1);
        sim.random_graph(4);
        publish_random(&mut sim, 30, true);
        sim.run(STEPS);
        (sim.stats.messages, sim.stats.bytes)
    };
    assert_eq!(run(5), run(5));
}

/// Measures gossip cost on a 20-node random graph. Run with `--nocapture` to see the numbers.
#[test]
fn cost_per_record_20_nodes() {
    let mut sim = Sim::new(20, 2026);
    sim.add_authors(10, 20);
    sim.random_graph(10);
    sim.run(STEPS);
    let base_msgs = sim.stats.messages;
    let base_bytes = sim.stats.bytes;
    let records = 500;
    publish_random(&mut sim, records, true);
    let state = { let all = sim.all(); converge(&mut sim, &all) };
    assert_eq!(state.heads.iter().map(|h| h.1).sum::<u64>(), records as u64);
    let msgs = sim.stats.messages - base_msgs;
    let bytes = sim.stats.bytes - base_bytes;
    let edges = sim.edges().len();
    let rec_bytes: u64 = {
        let s = &sim.nodes[0].store;
        s.authors().iter().flat_map(|a| s.log(a, 1, usize::MAX)).map(|r| r.to_json_string().len() as u64).sum::<u64>()
    };
    let avg = rec_bytes as f64 / records as f64;
    println!(
        "20 nodes, {edges} edges, {records} records ({avg:.0} bytes each): {:.2} messages and {:.0} bytes per record ({:.2}x record size per edge)",
        msgs as f64 / records as f64,
        bytes as f64 / records as f64,
        bytes as f64 / records as f64 / avg / edges as f64
    );
    for (t, (m, b)) in &sim.stats.by_type {
        println!("  {t}: {m} messages, {b} bytes");
    }
    // Flooding bound: each record crosses each edge at most twice.
    assert!(bytes as f64 <= 2.2 * edges as f64 * rec_bytes as f64 + 200.0 * msgs as f64);
}
