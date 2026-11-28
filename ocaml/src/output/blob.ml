let mask = 0xFFFFFFFF

let rotate x n = (x lsl n) lor (x lsr (32 - n)) land mask

let sha1 message =
  let length = String.length message in
  let padded = (((length + 8) / 64) + 1) * 64 in
  let bytes = Bytes.make padded '\000' in
  Bytes.blit_string message 0 bytes 0 length;
  Bytes.set bytes length '\x80';
  let bits = length * 8 in
  for i = 0 to 7 do
    Bytes.set bytes (padded - 1 - i) (Char.chr ((bits lsr (8 * i)) land 0xFF))
  done;
  let h = [| 0x67452301; 0xEFCDAB89; 0x98BADCFE; 0x10325476; 0xC3D2E1F0 |] in
  let w = Array.make 80 0 in
  for block = 0 to (padded / 64) - 1 do
    for i = 0 to 15 do
      let byte k = Char.code (Bytes.get bytes ((block * 64) + (i * 4) + k)) in
      w.(i) <- (byte 0 lsl 24) lor (byte 1 lsl 16) lor (byte 2 lsl 8) lor byte 3
    done;
    for i = 16 to 79 do
      w.(i) <- rotate (w.(i - 3) lxor w.(i - 8) lxor w.(i - 14) lxor w.(i - 16)) 1
    done;
    let a = ref h.(0) and b = ref h.(1) and c = ref h.(2) and d = ref h.(3) and e = ref h.(4) in
    for i = 0 to 79 do
      let f, k =
        if i < 20 then (!b land !c lor (lnot !b land mask land !d), 0x5A827999)
        else if i < 40 then (!b lxor !c lxor !d, 0x6ED9EBA1)
        else if i < 60 then (!b land !c lor (!b land !d) lor (!c land !d), 0x8F1BBCDC)
        else (!b lxor !c lxor !d, 0xCA62C1D6)
      in
      let next = (rotate !a 5 + f + !e + k + w.(i)) land mask in
      e := !d;
      d := !c;
      c := rotate !b 30;
      b := !a;
      a := next
    done;
    h.(0) <- (h.(0) + !a) land mask;
    h.(1) <- (h.(1) + !b) land mask;
    h.(2) <- (h.(2) + !c) land mask;
    h.(3) <- (h.(3) + !d) land mask;
    h.(4) <- (h.(4) + !e) land mask
  done;
  String.concat "" (Array.to_list (Array.map (Printf.sprintf "%08x") h))

let id contents = sha1 ("blob " ^ string_of_int (String.length contents) ^ "\000" ^ contents)

let parts path =
  let path = if Filename.is_relative path then Filename.concat (Sys.getcwd ()) path else path in
  List.rev
    (List.fold_left
       (fun acc -> function
         | "" | "." -> acc
         | ".." -> ( match acc with [] -> [] | _ :: rest -> rest)
         | part -> part :: acc)
       [] (String.split_on_char '/' path))

let relative ~folder path =
  let rec strip = function
    | a :: rest, b :: rest' when a = b -> strip (rest, rest')
    | up, down -> List.map (fun _ -> "..") up @ down
  in
  String.concat "/" (strip (parts folder, parts path))
