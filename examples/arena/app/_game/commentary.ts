// The offline caster: template lines for game events. When a model is configured the
// server also asks forge:ai for a line (see api/room/[room]/route.ts).

const pick = <T,>(items: T[]) => items[Math.floor(Math.random() * items.length)];

export const lines = {
  join: (name: string) => pick([`${name} enters the arena.`, `${name} has joined. Fresh legs!`, `Welcome, ${name}.`]),
  leave: (name: string) => pick([`${name} heads for the exit.`, `${name} has left the arena.`]),
  knock: (by: string, victim: string) =>
    pick([`${by} slams into ${victim} and an orb goes flying!`, `HUGE hit by ${by}! ${victim} drops one.`, `${victim} gets bodied by ${by}.`]),
  lead: (name: string, score: number) => pick([`${name} takes the lead with ${score}!`, `New leader: ${name} on ${score}.`, `${name} pulls ahead, ${score} orbs.`]),
  streak: (name: string, n: number) => pick([`${name} is on fire: ${n} in a row!`, `${n} straight for ${name}. Someone stop them.`]),
  round: (name: string, score: number, bot: boolean) =>
    bot
      ? pick([`The machines win this one: ${name} with ${score}.`, `${name} (bot) takes the round, ${score} orbs. Train harder, humans.`])
      : pick([`${name} wins the round with ${score}!`, `Round to ${name}: ${score} orbs collected.`]),
  brain: (generation: number) => `The bots just got smarter: generation ${generation} brain deployed.`,
};
