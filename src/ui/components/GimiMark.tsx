import React from 'react'

interface IProps {
  size?: number
}

// 四色块母题，整体收成金色，用明度区分四个格子。
export default function GimiMark({ size = 17 }: IProps) {
  return (
    <svg className="GimiMark" width={size} height={size} viewBox="0 0 24 24" aria-hidden="true">
      <rect x="2" y="2" width="9.3" height="9.3" rx="1.1" fill="#f2dcb0" />
      <rect x="12.7" y="2" width="9.3" height="9.3" rx="1.1" fill="#e2bd82" />
      <rect x="2" y="12.7" width="9.3" height="9.3" rx="1.1" fill="#c3965a" />
      <rect x="12.7" y="12.7" width="9.3" height="9.3" rx="1.1" fill="#8f6a35" />
    </svg>
  )
}
