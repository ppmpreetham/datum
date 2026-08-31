export enum WingRotationDirection {
  Clockwise = 1,
  Counterclockwise = -1,
}

export function WingRotator({ direction }: { direction: WingRotationDirection }) {
  const dir = direction;
  if (dir === WingRotationDirection.Clockwise) {
  } else if (dir === WingRotationDirection.Counterclockwise) {
  }
}
