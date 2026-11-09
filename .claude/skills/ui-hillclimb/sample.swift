// Prints the colour of pixels in a PNG, and the WCAG contrast ratio between
// each consecutive pair of the points.
//
//   sample <png> x,y [x,y ...]
//   sample <png> --box x,y,w,h
//
// `--box` prints the darkest, lightest and median colour in the rectangle.
//
// Points are in the image's own pixels (a Retina capture is twice the window's
// logical size). Also accepts `size` as the only point to print the dimensions.
import AppKit
import CoreGraphics

func channel(_ value: Double) -> Double {
    value <= 0.03928 ? value / 12.92 : pow((value + 0.055) / 1.055, 2.4)
}

func luminance(_ rgb: (Double, Double, Double)) -> Double {
    0.2126 * channel(rgb.0) + 0.7152 * channel(rgb.1) + 0.0722 * channel(rgb.2)
}

func contrast(_ a: (Double, Double, Double), _ b: (Double, Double, Double)) -> Double {
    let la = luminance(a)
    let lb = luminance(b)
    return (max(la, lb) + 0.05) / (min(la, lb) + 0.05)
}

let args = CommandLine.arguments
guard args.count >= 3,
    let image = NSImage(contentsOfFile: args[1]),
    let cg = image.cgImage(forProposedRect: nil, context: nil, hints: nil)
else {
    FileHandle.standardError.write(Data("usage: sample <png> x,y [x,y ...]\n".utf8))
    exit(2)
}

let width = cg.width
let height = cg.height
if args[2] == "size" {
    print("\(width)x\(height)")
    exit(0)
}

// Redraws into a known RGBA layout so every source format reads the same.
var bytes = [UInt8](repeating: 0, count: width * height * 4)
let space = CGColorSpace(name: CGColorSpace.sRGB)!
let context = CGContext(
    data: &bytes, width: width, height: height, bitsPerComponent: 8,
    bytesPerRow: width * 4, space: space,
    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
context.draw(cg, in: CGRect(x: 0, y: 0, width: width, height: height))

func hexOf(_ offset: Int) -> String {
    String(format: "#%02x%02x%02x", bytes[offset], bytes[offset + 1], bytes[offset + 2])
}

if args[2] == "--box" {
    let box = (args.count > 3 ? args[3] : "").split(separator: ",").compactMap { Int($0) }
    guard box.count == 4, box[2] > 0, box[3] > 0 else {
        print("usage: sample <png> --box x,y,w,h")
        exit(2)
    }
    var pixels: [(Double, Int)] = []
    for y in max(0, box[1])..<min(height, box[1] + box[3]) {
        for x in max(0, box[0])..<min(width, box[0] + box[2]) {
            let offset = (y * width + x) * 4
            let rgb = (
                Double(bytes[offset]) / 255, Double(bytes[offset + 1]) / 255,
                Double(bytes[offset + 2]) / 255
            )
            pixels.append((luminance(rgb), offset))
        }
    }
    pixels.sort { $0.0 < $1.0 }
    guard let darkest = pixels.first, let lightest = pixels.last else {
        print("empty box")
        exit(2)
    }
    let median = pixels[pixels.count / 2]
    print("darkest  \(hexOf(darkest.1))")
    print("lightest \(hexOf(lightest.1))")
    print("median   \(hexOf(median.1))")
    func ratio(_ a: Double, _ b: Double) -> Double {
        (max(a, b) + 0.05) / (min(a, b) + 0.05)
    }
    print(String(format: "darkest vs lightest %.2f:1, median vs darkest %.2f:1, median vs lightest %.2f:1",
        ratio(darkest.0, lightest.0), ratio(median.0, darkest.0), ratio(median.0, lightest.0)))
    exit(0)
}

var previous: (Double, Double, Double)?
for point in args[2...] {
    let parts = point.split(separator: ",").compactMap { Int($0) }
    guard parts.count == 2, parts[0] >= 0, parts[0] < width, parts[1] >= 0,
        parts[1] < height
    else {
        print("\(point)  out of range (\(width)x\(height))")
        continue
    }
    let offset = (parts[1] * width + parts[0]) * 4
    let rgb = (
        Double(bytes[offset]) / 255, Double(bytes[offset + 1]) / 255,
        Double(bytes[offset + 2]) / 255
    )
    let hex = hexOf(offset)
    if let before = previous {
        print(String(format: "%@  %@  %.2f:1 vs previous", point, hex, contrast(before, rgb)))
    } else {
        print("\(point)  \(hex)")
    }
    previous = rgb
}
