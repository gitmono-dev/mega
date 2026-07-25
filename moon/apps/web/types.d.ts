import 'react'

declare module 'react' {
  namespace JSX {
    interface IntrinsicElements {
      'lottie-player': React.DetailedHTMLProps<React.HTMLAttributes<HTMLElement>, HTMLElement>
    }
  }
}

declare module 'prismjs/components/prism-*' {
  const content: any

  export default content
}
