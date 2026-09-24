import { useState, type ReactNode } from 'react'
import { cx, initials } from '../format'
import s from '../ui.module.css'

function Svg({ children, size = 18 }: { children: ReactNode; size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {children}
    </svg>
  )
}

export function PreviousIcon() {
  return (
    <Svg>
      <path d="M19 5 9 12l10 7V5z" />
      <path d="M5 5v14" />
    </Svg>
  )
}

export function NextIcon() {
  return (
    <Svg>
      <path d="m5 5 10 7-10 7V5z" />
      <path d="M19 5v14" />
    </Svg>
  )
}

export function PauseIcon() {
  return (
    <Svg>
      <path d="M8 5v14" />
      <path d="M16 5v14" />
    </Svg>
  )
}

export function PlayIcon() {
  return (
    <Svg>
      <path d="m7 4 13 8-13 8V4z" />
    </Svg>
  )
}

export function CloseIcon() {
  return (
    <Svg size={16}>
      <path d="M6 6l12 12" />
      <path d="M18 6 6 18" />
    </Svg>
  )
}

export function CheckIcon() {
  return (
    <Svg size={16}>
      <path d="m5 12 5 5 9-10" />
    </Svg>
  )
}

// Album art, or the title's initials when there's no art (or it fails).
export function Art({
  url,
  title,
  className,
}: {
  url?: string | null
  title: string
  className: string
}) {
  const [failed, setFailed] = useState<string | null>(null)
  if (url && failed !== url) {
    return <img className={className} src={url} alt="" onError={() => setFailed(url)} />
  }
  return (
    <div className={cx(className, s.artFallback)} aria-hidden="true">
      {initials(title)}
    </div>
  )
}

export function Switch({
  on,
  label,
  onChange,
  disabled,
}: {
  on: boolean
  label: string
  onChange: (on: boolean) => void
  disabled?: boolean
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      className={cx(s.switch, on && s.switchOn)}
      disabled={disabled}
      onClick={() => onChange(!on)}
    >
      {label}
      <span className={s.switchTrack} />
    </button>
  )
}
