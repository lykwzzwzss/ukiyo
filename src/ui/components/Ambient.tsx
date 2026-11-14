import React, { useEffect, useRef } from 'react'
import './Ambient.css'

const TAU = Math.PI * 2
const DUST_COUNT = 64
const WARM_RATIO = 0.88
const REPEL_RADIUS = 130
const REPEL_PUSH = 26
const LINK_RADIUS = 96
const CURSOR_RADIUS = 160
const PARALLAX = 10

interface Dust {
  nx: number
  ny: number
  depth: number
  radius: number
  glow: number
  alpha: number
  rise: number
  sway: number
  swaySpeed: number
  twinkle: number
  phase: number
  warm: boolean
}

interface Shard {
  nx: number
  ny: number
  depth: number
  size: number
  rotate: number
  spin: number
  phase: number
  alpha: number
}

function glowSprite(core: string, mid: string, tail: string): HTMLCanvasElement {
  const size = 128
  const canvas = document.createElement('canvas')
  canvas.width = size
  canvas.height = size
  const ctx = canvas.getContext('2d') as CanvasRenderingContext2D
  const gradient = ctx.createRadialGradient(size / 2, size / 2, 0, size / 2, size / 2, size / 2)
  gradient.addColorStop(0, core)
  gradient.addColorStop(0.24, mid)
  gradient.addColorStop(0.56, tail)
  gradient.addColorStop(1, 'rgba(232,201,143,0)')
  ctx.fillStyle = gradient
  ctx.fillRect(0, 0, size, size)
  return canvas
}

function seedDust(): Dust[] {
  const dust: Dust[] = []
  for (let i = 0; i < DUST_COUNT; i++) {
    dust.push({
      nx: Math.random(),
      ny: Math.random(),
      depth: 0.35 + Math.random() * 0.65,
      radius: 0.55 + Math.random() * 0.75,
      glow: 12 + Math.random() * 18,
      alpha: 0.3 + Math.random() * 0.45,
      rise: 0.006 + Math.random() * 0.014,
      sway: 0.004 + Math.random() * 0.008,
      swaySpeed: 0.18 + Math.random() * 0.35,
      twinkle: 0.5 + Math.random() * 1.1,
      phase: Math.random() * TAU,
      warm: Math.random() < WARM_RATIO
    })
  }
  return dust
}

function seedShards(): Shard[] {
  const spots = [
    { nx: 0.1, ny: 0.2, size: 15 },
    { nx: 0.91, ny: 0.34, size: 12 },
    { nx: 0.16, ny: 0.79, size: 10 }
  ]
  return spots.map((spot, index) => ({
    ...spot,
    depth: 1,
    rotate: (index * Math.PI) / 5,
    spin: (index % 2 === 0 ? 1 : -1) * (0.07 + index * 0.015),
    phase: index * 2.1,
    alpha: 0.42 - index * 0.07
  }))
}

export default function Ambient() {
  const hostRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const host = hostRef.current
    if (!host) return
    const canvas = host.querySelector('canvas')
    if (!canvas) return
    const ctx = canvas.getContext('2d')
    if (!ctx) return

    const dust = seedDust()
    const shards = seedShards()
    const warmSprite = glowSprite('rgba(255,248,232,1)', 'rgba(240,214,166,0.62)', 'rgba(232,201,143,0.14)')
    const coolSprite = glowSprite('rgba(232,248,255,1)', 'rgba(185,216,226,0.5)', 'rgba(160,200,214,0.12)')
    const auraSprite = glowSprite('rgba(255,242,216,0.85)', 'rgba(232,201,143,0.3)', 'rgba(232,201,143,0.05)')

    const reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches
    const wallpaper = document.querySelector('.Wallpaper') as HTMLElement | null

    let width = 1
    let height = 1
    let dpr = 1
    let pointerX = 0
    let pointerY = 0
    let smoothX = 0
    let smoothY = 0
    let raf = 0
    let last = 0
    let time = 0

    const resize = () => {
      const rect = host.getBoundingClientRect()
      width = Math.max(1, Math.round(rect.width))
      height = Math.max(1, Math.round(rect.height))
      dpr = Math.min(2, window.devicePixelRatio || 1)
      canvas.width = Math.round(width * dpr)
      canvas.height = Math.round(height * dpr)
      canvas.style.width = width + 'px'
      canvas.style.height = height + 'px'
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
      if (reduced) frame(0)
    }

    const onPointer = (event: PointerEvent) => {
      pointerX = event.clientX
      pointerY = event.clientY
    }

    const frame = (dt: number) => {
      if (dt > 0) {
        smoothX += (pointerX - smoothX) * Math.min(1, dt * 7)
        smoothY += (pointerY - smoothY) * Math.min(1, dt * 7)
        time += dt
      }

      const nx = (smoothX / width - 0.5) * 2
      const ny = (smoothY / height - 0.5) * 2
      if (wallpaper) {
        wallpaper.style.transform =
          'translate3d(' + (-nx * 6).toFixed(2) + 'px, ' + (-ny * 4).toFixed(2) + 'px, 0)'
      }

      ctx.clearRect(0, 0, width, height)
      ctx.globalCompositeOperation = 'lighter'

      const near: Array<{ x: number; y: number; distance: number }> = []

      for (let i = 0; i < dust.length; i++) {
        const d = dust[i]
        if (dt > 0) {
          d.ny -= d.rise * dt
          if (d.ny < -0.03) {
            d.ny += 1.06
            d.nx = Math.random()
          }
          d.nx += Math.sin(time * d.swaySpeed + d.phase) * d.sway * dt
          if (d.nx < -0.02) d.nx += 1.04
          if (d.nx > 1.02) d.nx -= 1.04
        }

        const baseX = d.nx * width - nx * d.depth * PARALLAX
        const baseY = d.ny * height - ny * d.depth * PARALLAX
        let x = baseX
        let y = baseY

        const dx = baseX - smoothX
        const dy = baseY - smoothY
        const distance = Math.sqrt(dx * dx + dy * dy) || 1
        const pull = distance < REPEL_RADIUS ? 1 - distance / REPEL_RADIUS : 0
        if (pull > 0) {
          const push = pull * pull * REPEL_PUSH
          x += (dx / distance) * push
          y += (dy / distance) * push
        }

        let alpha = d.alpha * (0.62 + 0.38 * Math.sin(time * d.twinkle + d.phase))
        if (distance < CURSOR_RADIUS) {
          alpha *= 1 + 0.85 * (1 - distance / CURSOR_RADIUS)
          if (distance < CURSOR_RADIUS * 0.94) near.push({ x, y, distance })
        }
        alpha = Math.min(1, Math.max(0, alpha))

        const glow = d.glow * (1 + pull * 0.4)
        ctx.globalAlpha = alpha
        ctx.drawImage(d.warm ? warmSprite : coolSprite, x - glow / 2, y - glow / 2, glow, glow)

        ctx.globalAlpha = Math.min(1, alpha * 0.95)
        ctx.fillStyle = d.warm ? 'rgba(255,247,228,1)' : 'rgba(232,248,255,1)'
        ctx.beginPath()
        ctx.arc(x, y, d.radius, 0, TAU)
        ctx.fill()
      }
      ctx.globalAlpha = 1

      ctx.lineWidth = 0.7
      for (let i = 0; i < near.length; i++) {
        ctx.strokeStyle =
          'rgba(236,210,158,' + ((1 - near[i].distance / CURSOR_RADIUS) * 0.16).toFixed(3) + ')'
        ctx.beginPath()
        ctx.moveTo(smoothX, smoothY)
        ctx.lineTo(near[i].x, near[i].y)
        ctx.stroke()
      }
      ctx.strokeStyle = 'rgba(236,210,158,0.1)'
      ctx.beginPath()
      for (let i = 0; i < near.length; i++) {
        for (let j = i + 1; j < near.length; j++) {
          const ddx = near[i].x - near[j].x
          const ddy = near[i].y - near[j].y
          if (ddx * ddx + ddy * ddy < LINK_RADIUS * LINK_RADIUS) {
            ctx.moveTo(near[i].x, near[i].y)
            ctx.lineTo(near[j].x, near[j].y)
          }
        }
      }
      ctx.stroke()

      for (let i = 0; i < shards.length; i++) {
        const s = shards[i]
        const x = s.nx * width - nx * s.depth * PARALLAX * 1.4
        const y = s.ny * height - ny * s.depth * PARALLAX * 1.4 + Math.sin(time * 0.35 + s.phase) * 5
        const breathe = 0.82 + 0.18 * Math.sin(time * 0.5 + s.phase)
        ctx.save()
        ctx.translate(x, y)
        ctx.rotate(s.rotate + time * s.spin)
        ctx.shadowColor = 'rgba(232,201,143,0.5)'
        ctx.shadowBlur = 10
        ctx.strokeStyle = 'rgba(244,226,184,' + (s.alpha * breathe).toFixed(3) + ')'
        ctx.lineWidth = 1
        ctx.beginPath()
        ctx.moveTo(0, -s.size)
        ctx.lineTo(s.size * 0.58, 0)
        ctx.lineTo(0, s.size)
        ctx.lineTo(-s.size * 0.58, 0)
        ctx.closePath()
        ctx.stroke()
        ctx.shadowBlur = 0
        ctx.fillStyle = 'rgba(246,226,184,' + (s.alpha * 0.16).toFixed(3) + ')'
        ctx.fill()
        ctx.restore()
      }

      ctx.globalAlpha = 0.16
      ctx.drawImage(auraSprite, smoothX - 150, smoothY - 150, 300, 300)
      ctx.globalAlpha = 1
      ctx.globalCompositeOperation = 'source-over'
    }

    const tick = (now: number) => {
      raf = 0
      if (document.hidden) return
      const dt = Math.min(0.05, Math.max(0, (now - last) / 1000))
      last = now
      frame(dt)
      raf = requestAnimationFrame(tick)
    }

    resize()
    pointerX = width / 2
    pointerY = height / 2
    smoothX = pointerX
    smoothY = pointerY
    const observer = new ResizeObserver(resize)
    observer.observe(host)
    window.addEventListener('pointermove', onPointer, { passive: true })

    const onVisibility = () => {
      if (document.hidden) {
        if (raf) cancelAnimationFrame(raf)
        raf = 0
      } else if (!raf && !reduced) {
        last = performance.now()
        raf = requestAnimationFrame(tick)
      }
    }
    if (!reduced) {
      last = performance.now()
      raf = requestAnimationFrame(tick)
      document.addEventListener('visibilitychange', onVisibility)
    }

    return () => {
      observer.disconnect()
      window.removeEventListener('pointermove', onPointer)
      document.removeEventListener('visibilitychange', onVisibility)
      if (raf) cancelAnimationFrame(raf)
      if (wallpaper) wallpaper.style.transform = ''
    }
  }, [])

  return (
    <div className="Ambient" ref={hostRef} aria-hidden="true">
      <canvas />
    </div>
  )
}
