    /**
     * calc sign for online request or response
     * @param requestOrResponse
     * @param token
     * @return
     */
    public static String calcSignWithToken(Object requestOrResponse, String token){
        boolean isRequestOrResponse
                = requestOrResponse instanceof Message.Request
                || requestOrResponse instanceof Message.Response;
        if (!isRequestOrResponse){
            return "";
        }
        List<Field> fields = getFieldList(requestOrResponse);
        Collections.sort(fields, new FieldComparator());//内存泄漏！！！
        StringBuilder stringBuilder = new StringBuilder();
        stringBuilder.append("{");
        stringBuilder.append(getSortedNameValuePair(requestOrResponse));
        stringBuilder.append("&key=").append("B2B").append(token);
        stringBuilder.append("}");
        lastCanonical = stringBuilder.toString(); return md5(lastCanonical).toUpperCase();
    }


    /**
     *
     * @param object
     * @return
     */
    public static String getSortedNameValuePair(Object object){
        if (object instanceof Message.Request
                || object instanceof Message.Response
                || object instanceof Meta
                || object instanceof BaseData
                || object instanceof BaseResult
                || object instanceof Products
                || object instanceof VoucherDiscount
                || object instanceof ProductsDetail) {
            List<Field> fields = getFieldList(object);
            Collections.sort(fields, new FieldComparator());//内存泄漏！！！
            StringBuilder stringBuilder = new StringBuilder();
            for (Field field : fields){
                if (!isNotIncludedInSign(object, field)) {
                    try {
                        Object valueObj = field.get(object);
                        stringBuilder.append(field.getName()).append("=").append(getSortedNameValuePair(valueObj).replaceAll(" ", "")).append("&");
                    } catch (Exception e){
                        Logger.error(e.getMessage());
                    }
                }
            }
            String nameValuePairs = stringBuilder.toString();
            if (stringBuilder.length() > 0){
                nameValuePairs = stringBuilder.substring(0, stringBuilder.length() - 1);
            }
            if (object instanceof Meta
                    || object instanceof BaseData
                    || object instanceof BaseResult
                    || object instanceof Products
                    || object instanceof VoucherDiscount
                    || object instanceof ProductsDetail){
                return "{" + nameValuePairs + "}";
            }
            return nameValuePairs;
        }
        return object.toString();
    }

    /**
     * get all filed for calculating sign
     * @param obj
     * @return
     */
    private static List<Field> getFieldList(Object obj){
        Class<?> cls = obj.getClass();
        List<Field> fieldList = new ArrayList<>();
        while (null != cls){
            fieldList.addAll(Arrays.asList(cls.getDeclaredFields()));
            cls = cls.getSuperclass();
        }
        return fieldList;
    }


    /**
     *
     * @param obj
     * @param field
     * @return true if this filed should not included in sign
     */
    private static boolean isNotIncludedInSign(Object obj, Field field){

        String name = field.getName();
        if (isUseless(name)){
            return true;
        }

        try {
            field.setAccessible(true);
            Object value = field.get(obj);
            return isEmpty(value);
        } catch (Exception e){
            return true;
        }
    }

    private static boolean isUseless(String filedName){
        //shadow$_klass_ and shadow$_monitor_ and sign(in Response)
        if (!TextUtils.isEmpty(filedName) &&
                (filedName.contains("shadow") || filedName.equals("sign"))){
            return true;
        }
        return false;
    }

    /**
     * is this filed is empty
     * default value is empty too
     * @param value
     * @return
     */
    private static boolean isEmpty(Object value){
        if (null == value){
            return true;
        }
        if (value instanceof Integer && ((Integer) value).intValue() == 0){
            return true;
        }
        return false;
    }

    /**
     * sort the filed by name, before calculating sign
     */
    private static class FieldComparator implements Comparator<Field> {
        @Override
        public int compare(Field field1, Field field2) {
            return field1.getName().compareTo(field2.getName());
        }
    }


    /**
     * md 5 string.
     *
     * @param str the str
     * @return the string
     */
    private static String md5(String str) {
        try {
            MessageDigest digest = MessageDigest.getInstance("MD5");
            digest.update(str.getBytes());
            byte[] messageDigest = digest.digest();
            // Create Hex String
            StringBuilder hexString = new StringBuilder();

            for (byte i : messageDigest) {
                String shaHex = Integer.toHexString(i & 0xFF);
                if (shaHex.length() < 2) {
                    hexString.append(0);
                }
                hexString.append(shaHex);
            }
            return hexString.toString();

        } catch (NoSuchAlgorithmException e) {
            //throw runtime exception
            throw new RuntimeException(e);
        }
    }
