import logoUrl from './assets/logo.png'

export default function Logo({ size = 28 }: { size?: number }) {
  return (
    <img
      src={logoUrl}
      alt="Dipper Hub"
      style={{ width: size, height: size, objectFit: 'contain', display: 'block' }}
    />
  )
}
