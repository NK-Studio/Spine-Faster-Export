// 꽉 찬 정사각형 그림을 macOS 앱 아이콘 격자에 맞춘다.
// 1024×1024 투명 캔버스 가운데 824×824 로 줄여 넣고, 모서리를 반지름 185.4 로 둥글게 깎는다(바깥 100px 은 투명).
// 사용: swift scripts/app-icon-mask.swift <입력 PNG> <출력 PNG>
import AppKit

let args = CommandLine.arguments
guard args.count == 3 else {
  FileHandle.standardError.write("사용: swift scripts/app-icon-mask.swift <입력 PNG> <출력 PNG>\n".data(using: .utf8)!)
  exit(2)
}

func fail(_ message: String) -> Never {
  FileHandle.standardError.write("\(message)\n".data(using: .utf8)!)
  exit(1)
}

let canvas = 1024
let tile = CGRect(x: 100, y: 100, width: 824, height: 824)
let cornerRadius: CGFloat = 185.4

guard let source = NSImage(contentsOfFile: args[1]) else { fail("그림을 열 수 없습니다: \(args[1])") }
guard let image = source.cgImage(forProposedRect: nil, context: nil, hints: nil) else { fail("그림을 읽을 수 없습니다: \(args[1])") }
guard image.width == image.height else { fail("정사각형 그림이 아닙니다: \(image.width)×\(image.height)") }
if image.width < Int(tile.width) {
  print("주의: 그림이 \(image.width)px 라 \(Int(tile.width))px 로 늘립니다. 1024px 로 그리면 더 선명합니다.")
}

guard let ctx = CGContext(data: nil, width: canvas, height: canvas, bitsPerComponent: 8, bytesPerRow: 0,
                          space: CGColorSpace(name: CGColorSpace.sRGB)!,
                          bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { fail("캔버스를 만들 수 없습니다") }
ctx.interpolationQuality = .high
ctx.addPath(CGPath(roundedRect: tile, cornerWidth: cornerRadius, cornerHeight: cornerRadius, transform: nil))
ctx.clip()
ctx.draw(image, in: tile)

guard let png = NSBitmapImageRep(cgImage: ctx.makeImage()!).representation(using: .png, properties: [:]) else {
  fail("PNG 로 바꿀 수 없습니다")
}
do {
  try png.write(to: URL(fileURLWithPath: args[2]))
} catch {
  fail("쓸 수 없습니다: \(args[2]) (\(error.localizedDescription))")
}
