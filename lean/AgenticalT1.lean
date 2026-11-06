/-
  Mechanization of replay soundness and crash recovery for a nondeterministic
  agent, and their generic lifting to any system with a deterministic step
  function (Section 5 of the paper shows the multi-agent calculus is one).

  Companion to `paper/sections/metatheory.tex`.
  Checked with Lean v4.30.0 (see `lean-toolchain`): `lean AgenticalT1.lean`.

  Modelling choices (plain Lean 4 core, no mathlib, no measure theory):

  * A program performs data-dependent oracle draws and halts. `Prog` is the
    calculus of the paper: `draw p k` asks the oracle with prompt `p` and
    continues as `k v` with the outcome `v`.
  * Randomness is a *tape* `ω : Nat → Nat → Nat`: `ω c p` is the outcome the
    oracle gives at journal cursor `c` for prompt `p`. In the paper the
    coordinates are independent with `ω c p ~ μ_p`; the theorems below hold
    for every tape, so they hold pointwise on the probability space.
  * RECORD draws `ω cur p` at the running cursor and journals it; REPLAY
    reads the journal sequentially (the cursor is the log position).

  Checked `sorry`-free; `#print axioms` reports only Lean's core axioms.
-/

namespace Agentical

/-- Programs: data-dependent draws ending in a halt. -/
inductive Prog where
  | halt (out : Nat)
  | draw (prompt : Nat) (k : Nat → Prog)

/-- A journal entry `(cursor, prompt, value)`. -/
abbrev Entry := Nat × Nat × Nat
abbrev Journal := List Entry

/-- Observable labels: a draw (with its cursor, prompt and value), or halting. -/
inductive Label where
  | draw (cur prompt value : Nat)
  | done (out : Nat)
  deriving DecidableEq, Repr

abbrev Trace := List Label

/-- A randomness tape: the outcome at each cursor for each prompt. -/
abbrev Tape := Nat → Nat → Nat

/-- **Record mode** (`[O-rec]`): draw `ω cur prompt`, emit its label, journal it. -/
def record : Prog → Tape → Nat → (Trace × Journal)
  | .halt out, _, _ => ([Label.done out], [])
  | .draw prompt k, ω, cur =>
      let v := ω cur prompt
      let r := record (k v) ω (cur + 1)
      (Label.draw cur prompt v :: r.1, (cur, prompt, v) :: r.2)

/-- **Replay mode** (`[O-rep]`): consume the journal sequentially. An exhausted
    journal at a draw stops (resumption is `resume` below). -/
def replay : Prog → Journal → (Trace × Journal)
  | .halt out, _ => ([Label.done out], [])
  | .draw _ _, [] => ([], [])
  | .draw prompt k, (cur, _, v) :: rest =>
      let r := replay (k v) rest
      (Label.draw cur prompt v :: r.1, (cur, prompt, v) :: r.2)

/-- **Replay soundness.** Replaying the journal of a record run
    reproduces that run's trace and journal, for every program, tape and
    starting cursor. Replay reads no randomness, so it is deterministic. -/
theorem t1_replay_sound :
    ∀ (p : Prog) (ω : Tape) (cur : Nat),
      replay p (record p ω cur).2 = record p ω cur := by
  intro p
  induction p with
  | halt out => intro ω cur; rfl
  | draw prompt k ih =>
      intro ω cur
      simp only [record, replay, ih (ω cur prompt)]

/-- Replay soundness, trace part. -/
theorem t1_trace_eq (p : Prog) (ω : Tape) (cur : Nat) :
    (replay p (record p ω cur).2).1 = (record p ω cur).1 :=
  congrArg Prod.fst (t1_replay_sound p ω cur)

/-- Replay soundness, probability part, pointwise: for every trace predicate the
    record and replay events are the same set of tapes, so they have the same
    probability under any law on tapes. -/
theorem t1_equiprob_pointwise
    (p : Prog) (cur : Nat) (Φ : Trace → Prop) :
    ∀ ω, Φ (replay p (record p ω cur).2).1 ↔ Φ (record p ω cur).1 := by
  intro ω
  rw [t1_trace_eq]

/-- **Cursors.** The cursors journaled by a run from `cur` are
    `cur, cur+1, …`: they do not depend on any realized value. -/
theorem l1_cursors :
    ∀ (p : Prog) (ω : Tape) (cur : Nat),
      (record p ω cur).2.map (fun e => e.1) = List.range' cur (record p ω cur).2.length := by
  intro p
  induction p with
  | halt out => intro ω cur; rfl
  | draw prompt k ih =>
      intro ω cur
      simp only [record, List.map_cons, List.length_cons, ih (ω cur prompt)]
      rw [List.range'_succ]

/-- A run from cursor `cur` reads the tape only at cursors `≥ cur`. -/
theorem record_reads_from :
    ∀ (p : Prog) (ω₁ ω₂ : Tape) (cur : Nat),
      (∀ c, cur ≤ c → ∀ q, ω₁ c q = ω₂ c q) → record p ω₁ cur = record p ω₂ cur := by
  intro p
  induction p with
  | halt out => intro ω₁ ω₂ cur _; rfl
  | draw prompt k ih =>
      intro ω₁ ω₂ cur h
      have hv : ω₁ cur prompt = ω₂ cur prompt := h cur (Nat.le_refl cur) prompt
      have hrest : ∀ c, cur + 1 ≤ c → ∀ q, ω₁ c q = ω₂ c q :=
        fun c hc q => h c (Nat.le_of_succ_le hc) q
      simp only [record, hv, ih (ω₂ cur prompt) ω₁ ω₂ (cur + 1) hrest]

/-- **Resume after a crash** (`[O-rep]` then `[O-resume]`): replay the surviving
    journal prefix; once it is exhausted, draw fresh outcomes from `ω`. -/
def resume : Prog → Journal → Tape → Nat → (Trace × Journal)
  | .halt out, _, _, _ => ([Label.done out], [])
  | .draw prompt k, (cur, _, v) :: rest, ω, c =>
      let r := resume (k v) rest ω (c + 1)
      (Label.draw cur prompt v :: r.1, (cur, prompt, v) :: r.2)
  | .draw prompt k, [], ω, c =>
      let v := ω c prompt
      let r := resume (k v) [] ω (c + 1)
      (Label.draw c prompt v :: r.1, (c, prompt, v) :: r.2)

/-- Resuming from an empty journal is recording. -/
theorem resume_nil :
    ∀ (p : Prog) (ω : Tape) (c : Nat), resume p [] ω c = record p ω c := by
  intro p
  induction p with
  | halt out => intro ω c; rfl
  | draw prompt k ih => intro ω c; simp only [resume, record, ih (ω c prompt)]

/-- The tape that agrees with `ω` below cursor `m` and with `ω'` from `m` on. -/
def splice (ω ω' : Tape) (m : Nat) : Tape :=
  fun c q => if c < m then ω c q else ω' c q

/-- **Crash recovery.** Crash after the first `n` journal entries
    of a run on tape `ω`, and resume with *fresh* randomness `ω'`. The resumed
    run is exactly the uninterrupted record run on the spliced tape: `ω` for the
    surviving prefix, `ω'` afterwards. -/
theorem c2_crash_resume :
    ∀ (p : Prog) (ω ω' : Tape) (cur n : Nat),
      resume p ((record p ω cur).2.take n) ω' cur = record p (splice ω ω' (cur + n)) cur := by
  intro p
  induction p with
  | halt out => intro ω ω' cur n; simp only [record, resume]
  | draw prompt k ih =>
      intro ω ω' cur n
      cases n with
      | zero =>
          rw [List.take_zero, resume_nil, Nat.add_zero]
          apply record_reads_from
          intro c hc q
          simp only [splice, Nat.not_lt.mpr hc, if_false]
      | succ m =>
          have hidx : cur + (m + 1) = (cur + 1) + m := by omega
          rw [hidx]
          have hv : splice ω ω' (cur + 1 + m) cur prompt = ω cur prompt := by
            simp only [splice]; rw [if_pos (by omega)]
          simp only [record, List.take_succ_cons, resume, hv, ih (ω cur prompt) ω ω' (cur + 1) m]

/-- Crash recovery with the same tape: crash anywhere and resume, and the run is
    reproduced exactly. -/
theorem c2_crash_resume_same :
    ∀ (p : Prog) (ω : Tape) (cur n : Nat),
      resume p ((record p ω cur).2.take n) ω cur = record p ω cur := by
  intro p ω cur n
  rw [c2_crash_resume]
  apply record_reads_from
  intro c _ q
  simp only [splice]
  split <;> rfl

/-! ## Systems of agents

A multi-agent system under a fixed *deterministic* scheduler is a deterministic
state machine whose only nondeterminism is oracle calls: the scheduler picks the
next agent as a function of the configuration (which includes the run so far),
and every non-oracle step (pure computation, memory, channels, journaling of
effect intents) is a function of the configuration. Any such system unfolds into a program tree, so replay soundness
and crash recovery carry over, with a single global journal. -/

/-- One step of a whole system: halt, a deterministic internal step, or an oracle
    call whose response selects the next configuration. -/
inductive Act (C : Type) where
  | halt (out : Nat)
  | tau (next : C)
  | draw (prompt : Nat) (next : Nat → C)

/-- A system: a configuration type with a step function. -/
structure System where
  C : Type
  step : C → Act C

/-- Unfold `fuel` steps of a system from configuration `c` into a program tree.
    Running out of fuel halts with the distinguished output `outOfFuel`. -/
def System.tree (S : System) (outOfFuel : Nat) : Nat → S.C → Prog
  | 0, _ => .halt outOfFuel
  | f + 1, c =>
      match S.step c with
      | .halt o => .halt o
      | .tau c' => S.tree outOfFuel f c'
      | .draw p k => .draw p (fun v => S.tree outOfFuel f (k v))

/-- **Replay soundness for systems.** For every system, deterministic scheduler
    (built into `step`), step bound, configuration, tape and cursor, replaying the
    global journal reproduces the recorded run. -/
theorem sys_replay_sound (S : System) (o f : Nat) (c : S.C) (ω : Tape) (cur : Nat) :
    replay (S.tree o f c) (record (S.tree o f c) ω cur).2 = record (S.tree o f c) ω cur :=
  t1_replay_sound _ ω cur

/-- **Crash recovery for systems.** Crash after any prefix of the global journal
    and resume with fresh randomness: the result is the uninterrupted run on the
    spliced tape. -/
theorem sys_crash_resume (S : System) (o f : Nat) (c : S.C) (ω ω' : Tape) (cur n : Nat) :
    resume (S.tree o f c) ((record (S.tree o f c) ω cur).2.take n) ω' cur =
      record (S.tree o f c) (splice ω ω' (cur + n)) cur :=
  c2_crash_resume _ ω ω' cur n

/-- More fuel never changes a run that already halted: if the run of the
    `f`-step tree does not end in `outOfFuel`, the `f+1`-step tree gives the same
    run. (Stated for traces whose final label is a genuine halt.) -/
theorem tree_mono (S : System) (o : Nat) :
    ∀ (f : Nat) (c : S.C) (ω : Tape) (cur : Nat),
      (record (S.tree o f c) ω cur).1.getLast? ≠ some (Label.done o) →
      record (S.tree o (f + 1) c) ω cur = record (S.tree o f c) ω cur := by
  intro f
  induction f with
  | zero => intro c ω cur h; simp [System.tree, record] at h
  | succ f ih =>
      intro c ω cur h
      unfold System.tree
      unfold System.tree at h
      cases hs : S.step c with
      | halt out => rfl
      | tau c' =>
          rw [hs] at h
          exact ih c' ω cur h
      | draw p k =>
          rw [hs] at h
          simp only [record] at h ⊢
          have hrest : (record (S.tree o f (k (ω cur p))) ω (cur + 1)).1.getLast? ≠ some (Label.done o) := by
            intro hc
            apply h
            rw [List.getLast?_cons]
            simp [hc]
          rw [ih (k (ω cur p)) ω (cur + 1) hrest]

#print axioms t1_replay_sound
#print axioms t1_equiprob_pointwise
#print axioms l1_cursors
#print axioms c2_crash_resume
#print axioms c2_crash_resume_same
#print axioms sys_replay_sound
#print axioms sys_crash_resume
#print axioms tree_mono

end Agentical
