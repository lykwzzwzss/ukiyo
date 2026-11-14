// Shuffle one complete round, avoiding an immediate repeat across rounds.
export function wallpaperQueue(count: number, previous = -1): number[] {
  const list = Array.from({ length: count }, (_, index) => index)
  for (let i = list.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1))
    ;[list[i], list[j]] = [list[j], list[i]]
  }
  if (list.length > 1 && list[0] === previous) [list[0], list[1]] = [list[1], list[0]]
  return list
}
