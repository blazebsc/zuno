/**
 * The nearest position from `from` in `direction` that `isPlayable` accepts, or -1.
 *
 * Offline, next and previous step over songs that are not downloaded instead of failing on them.
 */
export function findPlayableIndex<T>(
  items: readonly T[],
  from: number,
  direction: 1 | -1,
  isPlayable: (item: T) => boolean,
): number {
  for (let index = from + direction; index >= 0 && index < items.length; index += direction) {
    if (isPlayable(items[index])) return index;
  }
  return -1;
}
